//! The core's runtime: what a client starts once and asks for threads. It owns the
//! database, the live config and the model, checks the config file in the background
//! and gives each active thread its own task.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{info, warn};

use super::{
    Launcher, ThreadEvent, ThreadInput, ThreadOutput, ThreadRunner,
    model_choice::{self, Choice, ModelChoice},
};
use crate::runtime::agent::{Input, Mailboxes};
use crate::{
    catalog::agents::{AgentTree, Agents},
    catalog::graph::{AGENT_FILE, NodeText, SKILL_FILE, read_node_text, save_node_text},
    catalog::modes,
    catalog::skills::{self, SkillInfo},
    chat::commands::{ContextReport, context_report},
    config::live::LiveConfig,
    config::{ApiKey, ContextPolicy, ProviderConfig, ProviderKind},
    config::{mcp_server_yaml, parse_mcp_server},
    init::paths::Paths,
    init::{self, create_private_dir},
    mcp::{McpInfo, McpServers, McpStatus},
    providers::Provider,
    release::update::{self, Channel, UpdateInfo},
    runtime::chat::thread_start::{FORK_INTRO, start_thread},
    state::{LocalUnsent, Message, State, Task, Thread},
    tools::{ToolKind, ToolScope},
    util::sync::lock,
};

/// Hand edits to config.yml are picked up within this time.
const CONFIG_CHECK_PERIOD: Duration = Duration::from_secs(60);
/// How long shutdown waits for stopped subagents to end.
const AGENTS_STOP_WAIT: Duration = Duration::from_secs(5);
/// How long the first chat waits for MCP servers still starting.
const MCP_START_WAIT: Duration = Duration::from_secs(10);

pub struct Runtime {
    launcher: Launcher,
    config_check: JoinHandle<()>,
    agents_check: JoinHandle<()>,
    model_discovery: Option<JoinHandle<()>>,
    threads: Mutex<JoinSet<()>>,
    /// Shipped files the user changed, kept at the start, with the new version beside.
    kept_builtin: Vec<String>,
}

/// A running thread, as its client sees it.
pub struct ThreadHandle {
    /// What `resume_thread` takes to pick the thread up later.
    pub thread_id: String,
    pub input: ThreadInput,
    pub output: ThreadOutput,
}

impl Runtime {
    /// Sets up missing files, opens the database and the config, and picks the first
    /// saved model without a network request. `state_db` replaces the default database
    /// path. First start and unavailable providers still leave local history accessible.
    pub async fn start(paths: Paths, state_db: Option<PathBuf>) -> Result<Self> {
        let initialized = init::initialize(&paths)?;
        let state = State::open(state_db.unwrap_or_else(|| paths.state_db())).await?;
        let config = Arc::new(LiveConfig::load(paths.config_file())?);
        let choice = match model_choice::configured(&config.current()) {
            Ok(choice) => choice,
            Err(error) => {
                warn!("configured provider is unavailable: {error:#}");
                None
            }
        };
        if let Some(choice) = &choice {
            info!("model {}", choice.name());
        } else {
            info!("no model selected; local history is available");
        }
        let config_check = tokio::spawn(Arc::clone(&config).watch(CONFIG_CHECK_PERIOD));
        let state = Arc::new(state);
        let agents = Agents::new(paths.agents_dir(), Arc::clone(&state));
        if let Err(error) = agents.scan().await {
            warn!("cannot read the agents: {error:#}");
        }
        let agents_check = tokio::spawn(Arc::clone(&agents).watch(CONFIG_CHECK_PERIOD));
        let launcher = Launcher {
            state,
            model: Arc::new(choice.map_or_else(ModelChoice::offline, ModelChoice::new)),
            config: Arc::clone(&config),
            paths,
            mailboxes: Arc::new(Mailboxes::default()),
            mcp: McpServers::start(&config.current()),
            agents,
        };
        let model_discovery = if !config.current().providers().is_empty() {
            let config = Arc::clone(&config);
            let model = Arc::clone(&launcher.model);
            let mailboxes = Arc::clone(&launcher.mailboxes);
            Some(tokio::spawn(async move {
                let revision = model.revision();
                let selected = if let Some(saved) = model.current() {
                    model_choice::refresh(&saved)
                        .await
                        .map(|metadata| model.refresh_model(&saved, metadata))
                } else {
                    model_choice::pick(&config.current(), None)
                        .await
                        .map(|choice| {
                            if model.set_if_empty_at_revision(choice, &revision) {
                                model.current()
                            } else {
                                None
                            }
                        })
                };
                match selected {
                    Ok(Some(selected)) => {
                        for mailbox in mailboxes.lock().await.values() {
                            mailbox.emit(ThreadEvent::ModelSelected {
                                name: selected.name(),
                                context_window: selected.model.context_window,
                            });
                            mailbox.send(Input::Sent);
                        }
                    }
                    Ok(None) => {}
                    Err(error) => warn!("model discovery is unavailable: {error:#}"),
                }
            }))
        } else {
            None
        };
        Ok(Self {
            launcher,
            config_check,
            agents_check,
            model_discovery,
            threads: Mutex::new(JoinSet::new()),
            kept_builtin: initialized.builtin.kept,
        })
    }

    /// Shipped prompts and skills the user changed: kept, with the new version beside
    /// each as `.new`, by their path in the config folder.
    pub fn kept_builtin(&self) -> &[String] {
        &self.kept_builtin
    }

    /// The newest version on the update channel; `channel` switches the channel first.
    pub async fn check_update(&self, channel: Option<Channel>) -> Result<UpdateInfo> {
        if let Some(channel) = channel {
            self.config()
                .update(|config| config.set_update_channel(channel))?;
        }
        update::check(self.config().current().update_channel()).await
    }

    /// Puts release `tag` in place of the running binary; it applies at the next start.
    pub async fn install_update(&self, tag: &str) -> Result<PathBuf> {
        let binary = update::install(tag).await?;
        info!("updated to {tag} at {}", binary.display());
        Ok(binary)
    }

    pub fn config(&self) -> &Arc<LiveConfig> {
        &self.launcher.config
    }

    /// The model threads talk to now, as `provider/model` and its details.
    pub fn model(&self) -> Option<Choice> {
        self.launcher.model.current()
    }

    /// The selected model's saved policy; None while no model is selected.
    pub fn context_policy(&self) -> Option<ContextPolicy> {
        let choice = self.model()?;
        Some(
            self.config()
                .current()
                .context_policy(&choice.provider_name, &choice.model.id),
        )
    }

    /// Saves a model-specific policy without contacting the provider. Returns its
    /// effective input limit, including the known model window when available.
    pub fn set_context_policy(&self, policy: ContextPolicy) -> Result<u64> {
        policy.validate()?;
        let choice = self
            .model()
            .context("select a model before changing its context")?;
        let config = self.config().update(|config| {
            config.set_context_policy(&choice.provider_name, &choice.model.id, policy);
        })?;
        Ok(choice.input_limit(&config))
    }

    pub fn input_limit(&self) -> u64 {
        let config = self.config().current();
        self.model()
            .map_or(config.limits().max_input_tokens, |choice| {
                choice.input_limit(&config)
            })
    }

    /// Whether the start is still looking for a model in the background.
    pub fn discovering_model(&self) -> bool {
        self.model_discovery
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    }

    /// Adds or replaces a provider in the private YAML config. A model can be chosen
    /// separately through `/model`; saving this setting makes no network request.
    pub fn connect_provider(
        &self,
        name: String,
        base_url: String,
        api_key: Option<String>,
    ) -> Result<()> {
        ensure!(!name.trim().is_empty(), "provider name must not be empty");
        ensure!(
            !name.contains('/') && !name.chars().any(char::is_control),
            "provider name is invalid"
        );
        let provider = ProviderConfig {
            kind: ProviderKind::OpenAiCompatible,
            base_url,
            api_key: ApiKey(api_key.unwrap_or_default()),
            inline_tags: false,
        };
        let adapter = Arc::new(Provider::new(&provider)?);
        self.launcher
            .config
            .update(|config| config.set_provider(name.clone(), provider))?;
        self.launcher.model.replace_provider(&name, adapter);
        Ok(())
    }

    /// Reads the agents folder now instead of at the next minute.
    pub async fn read_agents(&self) -> Result<()> {
        self.launcher.agents.scan().await
    }

    /// The chat and the agents it can reach, as deep as they may ask each other.
    pub fn agent_tree(&self) -> AgentTree {
        self.launcher.agents.tree(&self.launcher.config.current())
    }

    pub fn set_agent_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        self.launcher
            .config
            .update(|config| config.set_agent_enabled(name, enabled))?;
        Ok(())
    }

    /// Stops one agent and every agent it asked; agents waiting for a free slot drop
    /// their waiting messages. The chat and other agents go on.
    pub async fn stop_agent(&self, thread_id: &str) -> Result<()> {
        let state = &self.launcher.state;
        let thread = state
            .load_thread(thread_id)
            .await?
            .with_context(|| format!("thread {thread_id} does not exist"))?;
        ensure!(
            thread.parent_thread_id.is_some(),
            "thread {thread_id} is not an agent's"
        );
        let mut threads = state.descendant_threads(thread_id).await?;
        threads.insert(0, thread_id.to_owned());
        for thread_id in threads {
            match self.launcher.mailboxes.get(&thread_id).await {
                Some(mailbox) => {
                    mailbox.send(Input::Cancel);
                }
                None => state.drop_pending(&thread_id).await?,
            }
        }
        info!("agent thread {thread_id} stopped by the user");
        Ok(())
    }

    /// What the next request of a chat thread carries; before a thread, the mode's prompt and the
    /// tools a chat gets.
    pub async fn context(&self, thread_id: Option<&str>) -> Result<ContextReport> {
        let launcher = &self.launcher;
        let config = launcher.config.current();
        let cwd = std::env::current_dir()?;
        let scope = ToolScope::new(&cwd, &cwd)?;
        let specs = launcher
            .tools(thread_id.unwrap_or_default(), None, 0)
            .specs(&scope, &config);
        let mode = config.mode().to_owned();
        let prompt = modes::prompt(&launcher.paths.modes_dir(), &mode)?;
        let limit = self
            .model()
            .map_or(config.limits().max_input_tokens, |choice| {
                choice.input_limit(&config)
            });
        context_report(&launcher.state, thread_id, &mode, prompt, &specs, limit).await
    }

    /// Every mode of the chat, in the order Tab walks them.
    pub fn modes(&self) -> Vec<String> {
        modes::list(&self.launcher.paths.modes_dir())
    }

    /// Moves the chat to the next mode; its prompt and tools apply from the next step.
    pub fn next_mode(&self) -> Result<String> {
        let current = self.config().current().mode().to_owned();
        let next = modes::next(&self.launcher.paths.modes_dir(), &current);
        self.config()
            .update(|config| config.set_mode(next.clone()))?;
        Ok(next)
    }

    /// An agent's AGENT.md as it is on disk; reading it again throws edits away.
    pub fn agent(&self, name: &str) -> Result<NodeText> {
        read_node_text(&self.launcher.paths.agents_dir(), AGENT_FILE, name)
    }

    /// Saves an agent's AGENT.md once the text reads as an agent, and reads the agents
    /// again so the new version applies now.
    pub async fn save_agent(&self, name: &str, text: &str) -> Result<NodeText> {
        let saved = save_node_text(&self.launcher.paths.agents_dir(), AGENT_FILE, name, text)?;
        self.read_agents().await?;
        Ok(saved)
    }

    pub fn set_tool_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        self.launcher.config.update(|config| {
            let mode = config.mode().to_owned();
            config.set_mode_tool_enabled(&mode, name, enabled);
        })?;
        Ok(())
    }

    pub fn set_memory_enabled(&self, enabled: bool) -> Result<()> {
        self.config()
            .update(|config| config.set_memory_enabled(enabled))?;
        Ok(())
    }

    pub fn set_update_channel(&self, channel: Channel) -> Result<()> {
        self.config()
            .update(|config| config.set_update_channel(channel))?;
        Ok(())
    }

    pub fn set_skill_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        self.launcher
            .config
            .update(|config| config.set_skill_enabled(name, enabled))?;
        Ok(())
    }

    pub fn skills(&self) -> Vec<SkillInfo> {
        skills::list(
            &self.launcher.paths.skills_dir(),
            &self.launcher.config.current(),
        )
    }

    /// A skill's file as it is on disk; reading it again throws edits away.
    pub fn skill(&self, name: &str) -> Result<NodeText> {
        read_node_text(&self.launcher.paths.skills_dir(), SKILL_FILE, name)
    }

    /// Saves a skill's file once the text reads as a skill.
    pub fn save_skill(&self, name: &str, text: &str) -> Result<NodeText> {
        save_node_text(&self.launcher.paths.skills_dir(), SKILL_FILE, name, text)
    }

    pub fn set_max_agents_at_work(&self, limit: usize) -> Result<()> {
        self.launcher
            .config
            .update(|config| config.set_max_agents_at_work(limit))?;
        Ok(())
    }

    pub fn set_max_goal_rounds(&self, rounds: u32) -> Result<()> {
        self.launcher
            .config
            .update(|config| config.set_max_goal_rounds(rounds))?;
        Ok(())
    }

    /// Switches every thread to `choice` from its next model call, and keeps it in the
    /// config.
    pub fn set_model(&self, choice: Choice) -> Result<()> {
        let name = choice.name();
        self.launcher
            .config
            .update(|config| config.set_model(name))?;
        self.launcher.model.set(choice.clone());
        let mailboxes = Arc::clone(&self.launcher.mailboxes);
        let selected = choice;
        tokio::spawn(async move {
            for mailbox in mailboxes.lock().await.values() {
                mailbox.emit(ThreadEvent::ModelSelected {
                    name: selected.name(),
                    context_window: selected.model.context_window,
                });
                mailbox.send(Input::Sent);
            }
        });
        Ok(())
    }

    pub fn paths(&self) -> &Paths {
        &self.launcher.paths
    }

    /// The tools a chat gets, by name.
    /// Every tool with its kind, by kind and then by name.
    pub fn tool_kinds(&self) -> Vec<(String, ToolKind)> {
        // Tools do not depend on the thread.
        self.launcher.tools("", None, 0).kinds()
    }

    /// How each MCP server in the config stands.
    pub fn mcp_statuses(&self) -> Vec<McpStatus> {
        self.launcher.mcp.statuses()
    }

    /// Every MCP server in the config, with how it stands.
    pub fn mcp_servers(&self) -> Vec<McpInfo> {
        self.launcher.mcp.infos(&self.launcher.config.current())
    }

    /// One MCP server as YAML to edit, `name:` and its fields.
    pub fn mcp_server_text(&self, name: &str) -> Result<String> {
        let config = self.launcher.config.current();
        let server = config
            .mcp()
            .get(name)
            .with_context(|| format!("no MCP server {name}"))?;
        mcp_server_yaml(name, server)
    }

    /// Saves one MCP server from its YAML in place of `old`, and starts it again. Empty
    /// text removes `old`. Returns the name saved.
    pub fn save_mcp_server(&self, old: Option<&str>, text: &str) -> Result<Option<String>> {
        if text.trim().is_empty() {
            let old = old.context("write the server's YAML")?;
            self.launcher
                .config
                .update(|config| config.set_mcp_server(old, None))?;
            self.launcher.mcp.restart(old, None);
            return Ok(None);
        }
        let (name, server) = parse_mcp_server(text)?;
        let config = self.launcher.config.current();
        ensure!(
            old == Some(name.as_str()) || !config.mcp().contains_key(&name),
            "MCP server {name} already exists"
        );
        self.launcher.config.update(|config| {
            if let Some(old) = old {
                config.set_mcp_server(old, None);
            }
            config.set_mcp_server(&name, Some(server.clone()));
        })?;
        if let Some(old) = old.filter(|old| *old != name) {
            self.launcher.mcp.restart(old, None);
        }
        self.launcher.mcp.restart(&name, Some(server));
        Ok(Some(name))
    }

    pub fn set_mcp_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        let config = self
            .launcher
            .config
            .update(|config| config.set_mcp_enabled(name, enabled))?;
        self.launcher
            .mcp
            .restart(name, config.mcp().get(name).cloned());
        Ok(())
    }

    /// Starts an MCP server again, as the config has it now.
    pub fn restart_mcp(&self, name: &str) {
        let config = self.launcher.config.current();
        self.launcher
            .mcp
            .restart(name, config.mcp().get(name).cloned());
    }

    /// Whether an MCP server is still starting.
    pub fn mcp_starting(&self) -> bool {
        self.launcher.mcp.starting()
    }

    /// Every thread, the most recently active first, for a client to offer.
    pub async fn list_threads(&self) -> Result<Vec<Thread>> {
        self.launcher.state.list_threads().await
    }

    /// The whole history of a thread, compacted messages included, oldest first.
    pub async fn thread_messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        self.launcher.state.list_messages(thread_id).await
    }

    /// Local messages waiting for an explicit resend. They are not model history.
    pub async fn local_unsent(&self, thread_id: &str) -> Result<Vec<LocalUnsent>> {
        self.launcher.state.local_unsent(thread_id).await
    }

    /// Explicitly queue one local message. Never called during startup or recovery.
    pub async fn resend_local_unsent(&self, thread_id: &str, id: &str) -> Result<()> {
        ensure!(self.model().is_some(), "provider is unavailable");
        self.launcher
            .state
            .promote_local_unsent(thread_id, id)
            .await?;
        if let Some(mailbox) = self.launcher.mailboxes.get(thread_id).await {
            mailbox.send(Input::Sent);
        }
        Ok(())
    }

    /// The task board an agent keeps in a thread.
    pub async fn thread_tasks(&self, thread_id: &str) -> Result<Vec<Task>> {
        self.launcher.state.thread_tasks(thread_id).await
    }

    /// Starts a thread whose tools work in `cwd`, and the task that runs it. The task
    /// ends once the client drops every `ThreadInput` and the queue is done.
    pub async fn new_thread(&self, source: &str, cwd: &Path) -> Result<ThreadHandle> {
        let mode = self.config().current().mode().to_owned();
        let thread =
            start_thread(&self.launcher.state, &self.launcher.paths, source, &mode).await?;
        info!(
            "thread {} started in {}",
            thread.thread_id,
            thread.workspace.display()
        );
        self.run_thread(thread.thread_id, &thread.workspace, cwd)
            .await
    }

    /// A new chat thread that starts from a /fork summary of another, after its prompt.
    pub async fn fork_thread(
        &self,
        source: &str,
        summary: &str,
        cwd: &Path,
    ) -> Result<ThreadHandle> {
        let mode = self.config().current().mode().to_owned();
        let thread =
            start_thread(&self.launcher.state, &self.launcher.paths, source, &mode).await?;
        let summary = format!("{FORK_INTRO}{summary}");
        self.launcher
            .state
            .add_system_message(&thread.thread_id, &summary)
            .await?;
        info!("thread {} forked", thread.thread_id);
        self.run_thread(thread.thread_id, &thread.workspace, cwd)
            .await
    }

    /// Picks a stored thread up again, with its tools working in `cwd`. What it left
    /// unfinished is closed first: a running turn ends as interrupted, and messages sent
    /// before a crash are taken in.
    pub async fn resume_thread(&self, thread_id: &str, cwd: &Path) -> Result<ThreadHandle> {
        let thread = self
            .launcher
            .state
            .load_thread(thread_id)
            .await?
            .with_context(|| format!("thread {thread_id} does not exist"))?;
        // An agent's thread runs as that agent, started by the core when it is written to.
        ensure!(
            thread.parent_thread_id.is_none(),
            "thread {thread_id} belongs to an agent"
        );
        let workspace = self.launcher.paths.thread_workspace(thread_id)?;
        create_private_dir(&workspace)?;
        info!("thread {thread_id} resumed in {}", workspace.display());
        self.run_thread(thread_id.to_owned(), &workspace, cwd).await
    }

    /// One task per thread: two would answer the same messages twice.
    async fn run_thread(
        &self,
        thread_id: String,
        workspace: &Path,
        cwd: &Path,
    ) -> Result<ThreadHandle> {
        // A chat opened right after the start still gets the tools of MCP servers.
        self.launcher.mcp.wait(MCP_START_WAIT).await;
        let mailboxes = Arc::clone(&self.launcher.mailboxes);
        let mut live = mailboxes.lock().await;
        ensure!(
            !live.contains_key(&thread_id),
            "thread {thread_id} is already open"
        );
        let setup = self.launcher.setup(&thread_id, cwd, workspace, None, 0)?;
        let (runner, input, output) = ThreadRunner::new(setup);
        live.insert(thread_id.clone(), runner.mailbox(&input));
        drop(live);
        let id = thread_id.clone();
        self.lock_threads().spawn(async move {
            runner.run().await;
            mailboxes.remove(&id).await;
        });
        Ok(ThreadHandle {
            thread_id,
            input,
            output,
        })
    }

    /// Lets every chat finish its queue, stops the agents still at work, stops the
    /// config check and closes the database.
    pub async fn shutdown(self) {
        let mut threads = std::mem::take(&mut *self.lock_threads());
        while threads.join_next().await.is_some() {}
        // No agent starts from here on; those at work are stopped until none is left,
        // since one may have started a moment before the stop.
        let mailboxes = &self.launcher.mailboxes;
        mailboxes.close();
        let started = Instant::now();
        while started.elapsed() < AGENTS_STOP_WAIT {
            let live = mailboxes.lock().await;
            if live.is_empty() {
                break;
            }
            for mailbox in live.values() {
                mailbox.send(Input::Cancel);
            }
            drop(live);
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.config_check.abort();
        self.agents_check.abort();
        if let Some(discovery) = self.model_discovery {
            discovery.abort();
        }
        self.launcher.state.close().await;
    }

    fn lock_threads(&self) -> MutexGuard<'_, JoinSet<()>> {
        lock(&self.threads)
    }
}
