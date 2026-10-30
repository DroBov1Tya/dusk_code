//! The core's runtime: what a client starts once and asks for threads. It owns the
//! database, the live config and the model, checks the config file in the background
//! and gives each active thread its own task.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{info, warn};

use super::{
    inbox::Input,
    launcher::Launcher,
    mailboxes::Mailboxes,
    model_choice::{self, Choice, ModelChoice},
    thread_runner::{ThreadEvent, ThreadInput, ThreadOutput, ThreadRunner},
};
use crate::{
    config::{ApiKey, ProviderConfig, ProviderKind},
    init::{self, create_private_dir},
    live_config::LiveConfig,
    mcp::{McpServers, McpStatus},
    paths::Paths,
    providers::Provider,
    state::{LocalUnsent, Message, State, Task, Thread},
    thread_start::start_thread,
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
    model_discovery: Option<JoinHandle<()>>,
    threads: Mutex<JoinSet<()>>,
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
        init::initialize(&paths)?;
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
        let launcher = Launcher {
            state: Arc::new(state),
            model: Arc::new(choice.map_or_else(ModelChoice::offline, ModelChoice::new)),
            config: Arc::clone(&config),
            paths,
            mailboxes: Arc::new(Mailboxes::default()),
            mcp: McpServers::start(&config.current()),
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
            model_discovery,
            threads: Mutex::new(JoinSet::new()),
        })
    }

    pub fn config(&self) -> &Arc<LiveConfig> {
        &self.launcher.config
    }

    /// The model threads talk to now, as `provider/model` and its details.
    pub fn model(&self) -> Option<Choice> {
        self.launcher.model.current()
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

    pub fn set_max_agents_at_work(&self, limit: usize) -> Result<()> {
        self.launcher
            .config
            .update(|config| config.set_max_agents_at_work(limit))?;
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
    pub fn tool_names(&self) -> Vec<String> {
        // Names do not depend on the thread.
        self.launcher
            .tools("", None, 0)
            .names()
            .map(str::to_owned)
            .collect()
    }

    /// How each MCP server in the config stands.
    pub fn mcp_statuses(&self) -> Vec<McpStatus> {
        self.launcher.mcp.statuses()
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

    pub async fn add_local_unsent(&self, thread_id: &str, content: &str) -> Result<LocalUnsent> {
        self.launcher
            .state
            .add_local_unsent(thread_id, content)
            .await
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
        let thread = start_thread(&self.launcher.state, &self.launcher.paths, source).await?;
        info!(
            "thread {} started in {}",
            thread.thread_id,
            thread.workspace.display()
        );
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
        let setup = self.launcher.setup(&thread_id, cwd, workspace, None, 0)?;
        let (runner, input, output) = ThreadRunner::new(setup);
        let mailboxes = Arc::clone(&self.launcher.mailboxes);
        {
            let mut live = mailboxes.lock().await;
            ensure!(
                !live.contains_key(&thread_id),
                "thread {thread_id} is already open"
            );
            live.insert(thread_id.clone(), runner.mailbox(&input));
        }
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
        if let Some(discovery) = self.model_discovery {
            discovery.abort();
        }
        self.launcher.state.close().await;
    }

    fn lock_threads(&self) -> MutexGuard<'_, JoinSet<()>> {
        lock(&self.threads)
    }
}

// Inserting into and taking from these cannot leave them half-changed, so a panic
// elsewhere does not make them unusable.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
