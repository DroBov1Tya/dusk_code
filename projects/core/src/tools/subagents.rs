//! Agents talking to agents. `ask_agent` hands a task to another agent, which works in a
//! thread of its own under the asking one, with its own system prompt, context and
//! tools, in the background: the call returns at once. Asking the same thread again
//! sends it more while it works. The agent writes back with `message_parent`, and its
//! final answer comes back on its own. Every message goes through the database, so
//! none is lost, and arrives between the reader's steps.
//!
//! A chat may ask the entry agents; an agent may ask the agents it links to. Any thread
//! may ask the general agent, which works like a chat: MAIN mode's prompt and every tool, so
//! delegating needs no AGENT.md. Depth is bounded, so agents cannot hand work down forever.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::info;

use super::{Progress, Tool, ToolContext, ToolKind};
use crate::{
    BoxFuture,
    agents::{GENERAL_AGENT, GENERAL_DESCRIPTION},
    runtime::{ThreadEvent, system::Launcher},
    state::Identity,
    state::Thread,
    thread_start::{self, display_name, read_prompt, start_agent_thread},
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskAgentArguments {
    agent: Option<String>,
    task: String,
    thread: Option<String>,
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

    async fn ask(
        &self,
        arguments: AskAgentArguments,
        cwd: &Path,
        progress: Option<Progress<'_>>,
    ) -> Result<String> {
        ensure!(!arguments.task.trim().is_empty(), "task is empty");
        let (thread_id, name, agent) = match &arguments.thread {
            Some(key) => {
                let thread = self.own_agent(key).await?;
                let agent = thread.agent.clone().unwrap_or_else(|| GENERAL_AGENT.into());
                if let Some(asked) = &arguments.agent {
                    ensure!(*asked == agent, "{key} works as {agent}, not {asked}");
                }
                (thread.id.clone(), display_name(&thread), agent)
            }
            None => self.start(arguments.agent.as_deref()).await?,
        };
        if let Some(progress) = progress {
            progress.emit(ThreadEvent::SubagentStarted {
                call_id: progress.call_id.to_owned(),
                thread_id: thread_id.clone(),
                agent: agent.clone(),
                task: arguments.task.clone(),
            });
        }
        self.launcher
            .deliver(&thread_id, &self.thread_id, &arguments.task, cwd)
            .await?;
        Ok(format!(
            "{name} (agent {agent}, thread {thread_id}) works on it in the background. Call \
             it {name} when you talk about it. What it writes and its final answer will come \
             to you as messages from {name}; to send it more, call ask_agent with thread \
             {name}."
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
            let (identity, prompt) = self.launcher.agents.prompt(agent_name).await?;
            info!("agent {agent_name} starts at version {}", identity.version);
            Some(prompt)
        };
        let state = &self.launcher.state;
        let thread = start_agent_thread(state, paths, &self.thread_id, agent_name, prompt).await?;
        let name = thread
            .name
            .unwrap_or_else(|| thread_start::agent_name(&thread.thread_id));
        Ok((thread.thread_id, name, agent_name.to_owned()))
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

    fn description(&self, _context: &ToolContext<'_>) -> String {
        let reachable = self.reachable();
        let mut agents: Vec<String> = reachable
            .iter()
            .take(MAX_LISTED)
            .map(|identity| {
                let description: String = identity
                    .description
                    .chars()
                    .take(MAX_LISTED_CHARS)
                    .collect();
                format!("- {}: {description}", identity.name)
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
             end your turn when there is nothing else to do until it answers.\nAgents:\n{}",
            agents.join("\n")
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "Agent name, as listed; general when left out."
                },
                "task": { "type": "string", "description": "Everything the agent needs." },
                "thread": {
                    "type": "string",
                    "description": "The name of an agent asked before, such as Fable-3a, \
                        or its thread, to write to it again."
                }
            },
            "required": ["task"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        let cwd = context.scope.cwd().to_owned();
        Box::pin(async move {
            let arguments: AskAgentArguments =
                serde_json::from_value(arguments).context("invalid arguments")?;
            self.ask(arguments, &cwd, context.progress).await
        })
    }
}

/// `message_parent`: an agent writes to the thread that asked it, while it works.
pub struct MessageParent {
    launcher: Launcher,
    thread_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MessageParentArguments {
    text: String,
}

impl MessageParent {
    pub fn new(launcher: Launcher, thread_id: String) -> Self {
        Self {
            launcher,
            thread_id,
        }
    }

    async fn send(&self, text: &str, cwd: &Path) -> Result<String> {
        ensure!(!text.trim().is_empty(), "text is empty");
        let thread = self.launcher.state.load_thread(&self.thread_id).await?;
        let parent = thread
            .and_then(|thread| thread.parent_thread_id)
            .context("this thread has no thread above it")?;
        self.launcher
            .deliver(&parent, &self.thread_id, text, cwd)
            .await?;
        Ok("sent".into())
    }
}

impl Tool for MessageParent {
    fn name(&self) -> &'static str {
        "message_parent"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Agents
    }

    fn description(&self, _context: &ToolContext<'_>) -> String {
        "Write to the agent that gave you your task, while you work: a question, a finding \
         it should know now, how far you are. It reads the message between its steps. Your \
         final answer reaches it on its own, so do not send that too."
            .into()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The message." }
            },
            "required": ["text"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        let cwd = context.scope.cwd().to_owned();
        Box::pin(async move {
            let arguments: MessageParentArguments =
                serde_json::from_value(arguments).context("invalid arguments")?;
            self.send(&arguments.text, &cwd).await
        })
    }
}
