//! `ask_agent`: hands a task to another agent, which works in a
//! thread of its own under the asking one, with its own system prompt, context and
//! tools, in the background: the call returns at once. Asking the same thread again
//! sends it more while it works. The agent writes back with `message_parent`, and its
//! final answer comes back on its own. Every message goes through the database, so
//! none is lost, and arrives between the reader's steps.
//!
//! A chat may ask the entry agents; an agent may ask the agents it links to. Any thread
//! may ask the general agent, which works like a chat: MAIN mode's prompt and every tool, so
//! delegating needs no AGENT.md. Depth is bounded, so agents cannot hand work down forever.

use crate::plugins::store::origin::{self, Origins};
use anyhow::{Context, Result, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use tracing::{info, warn};

use crate::tools::{Tool, ToolContext, ToolKind, schema};
use crate::{
    BoxFuture,
    catalog::agents::{GENERAL_AGENT, GENERAL_DESCRIPTION},
    catalog::skills,
    config::Effort,
    runtime::thread::start::{self, display_name, read_prompt, start_agent_thread},
    runtime::{
        Launcher, ThreadEvent,
        tiers::{self, Tier},
    },
    state::Identity,
    state::Thread,
};

/// Each agent listed costs tokens in every request.
const MAX_LISTED: usize = 40;
const MAX_LISTED_CHARS: usize = 300;
/// Agent threads one thread may open; asking an earlier one again is not limited.
const MAX_AGENTS_PER_THREAD: usize = 16;
/// Agent threads in one chat's whole tree: the per-thread cap alone allows thousands
/// three levels down.
const MAX_AGENTS_PER_CHAT: usize = 32;

pub struct AskAgent {
    launcher: Launcher,
    /// The asking thread, which becomes the parent of the agent's thread.
    thread_id: String,
    /// The agent the asking thread works as; `None` for a chat.
    asker: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AskAgentArguments {
    /// Agent name, as listed; general when left out.
    agent: Option<String>,
    /// Everything the agent needs.
    task: String,
    /// The name of an agent asked before, such as Fable-3a, or its thread, to write to it
    /// again.
    thread: Option<String>,
    /// How hard the agent reasons: low for lookups and plain edits, medium when left out,
    /// high or above for hard design, debugging or review. It keeps the level until
    /// asked with another; a model that cannot reason ignores it.
    effort: Option<Effort>,
    /// The model it answers on: strong for design, review and hard debugging, weak for
    /// mechanical work and read-only scouting, default otherwise. A tier the user did not
    /// set is this chat's model. It keeps the tier until asked with another.
    tier: Option<Tier>,
    /// A new agent works on a git branch of its own, made from your current branch in a
    /// worktree beside the repository, so its changes stay apart from yours until you
    /// merge them with `branch` (action merge, agent <its name>).
    isolated: Option<bool>,
}

impl AskAgent {
    pub fn new(launcher: Launcher, thread_id: String, asker: Option<String>) -> Self {
        Self {
            launcher,
            thread_id,
            asker,
        }
    }

    /// The agents this thread may ask: the entries for a chat, the links for an agent,
    /// those switched on.
    fn reachable(&self) -> Vec<Identity> {
        let config = self.launcher.config.current();
        self.launcher
            .agents
            .reachable(self.asker.as_deref(), &config)
    }

    /// The general agent has every tool, so an agent limited to some tools may not ask
    /// it: that would give it the rest.
    fn builtin_general(&self) -> bool {
        let config = self.launcher.config.current();
        let limited = self.asker.as_deref().is_some_and(|asker| {
            self.launcher
                .agents
                .get(asker)
                .is_some_and(|identity| identity.tools.is_some())
        });
        !limited && self.launcher.agents.builtin_general(&config)
    }

    async fn ask(&self, arguments: AskAgentArguments, context: &ToolContext) -> Result<String> {
        let (scope, progress) = (&*context.scope, context.progress.clone());
        ensure!(!arguments.task.trim().is_empty(), "task is empty");
        let isolated = arguments.isolated.unwrap_or(false);
        ensure!(
            !(isolated && arguments.thread.is_some()),
            "`isolated` is for a new agent; an agent at work keeps where it works"
        );
        if isolated {
            let repository = crate::project::git::repository(scope.cwd()).await;
            ensure!(
                repository.is_some_and(|repository| repository.branch.is_some()),
                "`isolated` needs a git repository with a branch checked out"
            );
            // Asked before the agent's thread is made, so a no leaves nothing behind.
            context
                .approve_git("give a new agent a branch of its own in a new worktree")
                .await?;
        }
        let cwd = scope.cwd();
        // Kept until the new agent is delivered its task, which gives it its slot.
        let mut admitted = None;
        let (thread_id, name, agent) = match &arguments.thread {
            Some(key) => {
                let thread = self.own_agent(key).await?;
                let agent = thread.agent.clone().unwrap_or_else(|| GENERAL_AGENT.into());
                if let Some(asked) = &arguments.agent {
                    ensure!(*asked == agent, "{key} works as {agent}, not {asked}");
                }
                (thread.id.clone(), display_name(&thread), agent)
            }
            None => {
                admitted = Some(self.launcher.admit_agent().await?);
                self.start(arguments.agent.as_deref()).await?
            }
        };
        let mut task = arguments.task.clone();
        let mut branched = String::new();
        if isolated {
            let state = &self.launcher.state;
            let made =
                crate::tools::start_branch(state, &thread_id, &self.thread_id, scope, Some(&name))
                    .await;
            // `isolated` is refused for an agent asked before, so this one is new.
            let made = match made {
                Ok(made) => made,
                Err(error) => {
                    self.forget_new(&thread_id).await;
                    return Err(error);
                }
            };
            task = format!(
                "You work on your own git branch {}, made from {}, in the worktree {}: your \
                 files and commands are there. Commit your work with the `branch` tool \
                 (action commit) before your final answer; the agent that asked you merges \
                 it.\n\n{task}",
                made.branch, made.base, made.dir
            );
            branched = format!(
                " It works on branch {} in {}; when its work is checked, merge it with \
                 `branch` (action merge, agent {name}).",
                made.branch, made.dir
            );
        }
        let state = &self.launcher.state;
        if let Some(effort) = arguments.effort {
            state.set_agent_effort(&thread_id, effort).await?;
        }
        let chat = self.launcher.model.current().map(|choice| choice.name());
        if arguments.tier.is_some() {
            let config = self.launcher.config.current();
            let model = tiers::model_for(arguments.tier, &config, chat.as_deref());
            state.set_agent_model(&thread_id, model.as_deref()).await?;
        }
        // A tier whose provider was removed since is the chat's model, as the runner finds.
        let config = self.launcher.config.current();
        let model = state
            .agent_model(&thread_id)
            .await?
            .filter(|name| tiers::connected(&config, name))
            .or(chat);
        // An agent at work goes on with its model; the one picked now comes at its next wake.
        let running = self.launcher.running_model(&thread_id).await;
        let later = running
            .as_ref()
            .and(model.as_ref())
            .filter(|picked| running.as_ref() != Some(*picked))
            .cloned();
        let model = running.or(model);
        let effort = Effort::for_subagent(config.effort(), state.agent_effort(&thread_id).await?);
        if let Some(progress) = progress {
            progress.emit(ThreadEvent::SubagentStarted {
                call_id: progress.call_id.to_owned(),
                thread_id: thread_id.clone(),
                agent: agent.clone(),
                task: task.clone(),
                effort,
                model: model.clone(),
                task_id: None,
            });
        }
        self.launcher
            .deliver(&thread_id, &self.thread_id, &task, cwd)
            .await?;
        drop(admitted);
        let effort = effort.map_or("the provider's default", Effort::name);
        let model = model.as_deref().unwrap_or("no model");
        let later = later.map_or_else(String::new, |later| {
            format!(" It is at work, so it switches to {later} only when it is woken next.")
        });
        Ok(format!(
            "{name} (agent {agent}, thread {thread_id}, model {model}, effort {effort}) works \
             on it in the background. Call it {name} when you talk about it. What it writes \
             and its final answer will come to you as messages from {name}; to send it more, \
             call ask_agent with thread {name}.{branched}{later}"
        ))
    }

    /// A new thread for `agent`, the general agent when `None`: its ID, name and agent.
    async fn start(&self, agent: Option<&str>) -> Result<(String, String, String)> {
        let agent_name = agent.unwrap_or(GENERAL_AGENT);
        let known = self
            .reachable()
            .iter()
            .any(|identity| identity.name == agent_name);
        let builtin = !known && agent_name == GENERAL_AGENT && self.builtin_general();
        ensure!(known || builtin, "no agent {agent_name} to ask from here");
        let state = &self.launcher.state;
        let chat = state.root_thread(&self.thread_id).await?;
        let in_chat = state.descendant_threads(&chat).await?.len();
        ensure!(
            in_chat < MAX_AGENTS_PER_CHAT,
            "this chat has {MAX_AGENTS_PER_CHAT} agents already; continue one of them with \
             `thread` or do the work yourself"
        );
        let opened = self.launcher.state.agent_threads(&self.thread_id).await?;
        ensure!(
            opened.len() < MAX_AGENTS_PER_THREAD,
            "this thread has opened {MAX_AGENTS_PER_THREAD} agent threads; continue one of \
             them with `thread` instead"
        );
        let paths = &self.launcher.paths;
        let prompt = if builtin {
            read_prompt(&paths.main_prompt())?
        } else {
            let config = self.launcher.config.current();
            let skills = skills::list(&paths.skills_dir(), &config);
            let (identity, prompt) = self.launcher.agents.prompt(agent_name, &skills).await?;
            info!("agent {agent_name} starts at version {}", identity.version);
            Some(prompt)
        };
        let state = &self.launcher.state;
        let thread = start_agent_thread(state, paths, &self.thread_id, agent_name, prompt).await?;
        let name = thread
            .name
            .unwrap_or_else(|| start::agent_name(&thread.thread_id));
        Ok((thread.thread_id, name, agent_name.to_owned()))
    }

    /// Erases an agent thread just made, with its folder, when it cannot be given its
    /// task: it would be left with nothing to do.
    async fn forget_new(&self, thread_id: &str) {
        let folders = match self.launcher.state.delete_thread(thread_id).await {
            Ok(folders) => folders,
            Err(error) => {
                warn!("cannot remove the agent thread {thread_id} that got no task: {error:#}");
                return;
            }
        };
        for folder in folders {
            let Ok(path) = self.launcher.paths.workspace_folder(&folder) else {
                continue;
            };
            match tokio::fs::remove_dir_all(&path).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    warn!("cannot remove the folder of agent thread {thread_id}: {error}")
                }
            }
        }
    }

    /// An agent this thread asked before, by its name or its thread. Nothing else is
    /// reachable: a thread ID from anywhere else is refused.
    async fn own_agent(&self, key: &str) -> Result<Thread> {
        let threads = self.launcher.state.agent_threads(&self.thread_id).await?;
        threads
            .into_iter()
            .find(|thread| thread.id == key || display_name(thread).eq_ignore_ascii_case(key))
            .with_context(|| format!("no agent {key} was asked from here"))
    }
}

impl Tool for AskAgent {
    fn name(&self) -> &'static str {
        "ask_agent"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Agents
    }

    fn description(&self, _context: &ToolContext) -> String {
        let reachable = self.reachable();
        let origins = Origins::load(&self.launcher.paths);
        let mut agents: Vec<String> = reachable
            .iter()
            .take(MAX_LISTED)
            .map(|identity| {
                let description: String = identity
                    .description
                    .chars()
                    .take(MAX_LISTED_CHARS)
                    .collect();
                let source = origins
                    .unverified("agents", &identity.name)
                    .map(|(_, source)| source);
                origin::listed(&identity.name, source, &description)
            })
            .collect();
        if reachable.len() > MAX_LISTED {
            agents.push(format!(
                "({} more not listed)",
                reachable.len() - MAX_LISTED
            ));
        }
        if self.builtin_general() {
            agents.insert(0, format!("- {GENERAL_AGENT}: {GENERAL_DESCRIPTION}"));
        }
        format!(
            "Give a task to a subagent, or send more to one at work. It works in its own \
             thread with its own context and tools, in the background, so a long search or \
             a self-contained part of the work does not fill this conversation, and several \
             can work at once. It sees nothing of this conversation, so describe the task \
             fully. Leave out `agent` for {GENERAL_AGENT}. The call returns at once with \
             the name the agent got, such as Fable-3a: use it for the agent, with the user \
             too. What the agent writes and its final answer arrive later as messages from \
             that name. Pass the name as `thread` to write to the same agent again; \
             end your turn when there is nothing else to do until it answers. A brief \
             gives the goal, the relevant paths, what is already known and what to \
             report. Use agents for large searches, independent review and parts that \
             would fill this context; do small things yourself. Pick `tier` by the work: \
             strong for design, review and hard debugging, weak for mechanical edits and \
             read-only scouting, default otherwise; a tier the user left unset is this \
             chat's model. Pick `effort` the same way. Set `isolated` for an agent \
             whose edits should stay on a branch of its own, such as parallel or risky \
             changes; merge it with `branch` once checked.\nAgents:\n{}",
            agents.join("\n")
        )
    }

    fn parameters(&self) -> Value {
        schema::of::<AskAgentArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: AskAgentArguments =
                serde_json::from_value(arguments).context("invalid arguments")?;
            self.ask(arguments, &context).await
        })
    }
}
