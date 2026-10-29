//! The core's runtime: what a client starts once and asks for threads. It owns the
//! database, the live config and the model, checks the config file in the background
//! and gives each active thread its own task.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use tokio::task::{JoinHandle, JoinSet};
use tracing::info;

use super::{
    inbox::Input,
    launcher::Launcher,
    mailboxes::Mailboxes,
    thread_runner::{ThreadInput, ThreadOutput, ThreadRunner},
};
use crate::{
    init::{self, create_private_dir},
    live_config::LiveConfig,
    paths::Paths,
    providers::{Model, ProviderRegistry},
    state::{Message, State, Thread},
    thread_start::start_thread,
};

/// Hand edits to config.yml are picked up within this time.
const CONFIG_CHECK_PERIOD: Duration = Duration::from_secs(60);
/// How long shutdown waits for stopped subagents to end.
const AGENTS_STOP_WAIT: Duration = Duration::from_secs(5);

pub struct Runtime {
    launcher: Launcher,
    config_check: JoinHandle<()>,
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
    /// configured provider and its first model. `state_db` replaces the default database
    /// path. On a first start the config has no provider yet; the error says where to
    /// add one.
    pub async fn start(paths: Paths, state_db: Option<PathBuf>) -> Result<Self> {
        if init::initialize(&paths)?.config_created {
            bail!(
                "created {}; add a provider and restart",
                paths.config_file().display()
            );
        }
        let state = State::open(state_db.unwrap_or_else(|| paths.state_db())).await?;
        let config = Arc::new(LiveConfig::load(paths.config_file())?);
        let providers = ProviderRegistry::from_config(&config.current())?;
        let provider_name = providers.names().next().context("no provider configured")?;
        let provider = providers
            .provider(provider_name)
            .context("configured provider is missing")?;
        let model = provider.default_model().await?;
        info!("provider {provider_name}, model {}", model.id);
        let config_check = tokio::spawn(Arc::clone(&config).watch(CONFIG_CHECK_PERIOD));
        let launcher = Launcher {
            state: Arc::new(state),
            provider,
            model,
            config,
            paths,
            mailboxes: Arc::new(Mailboxes::default()),
        };
        Ok(Self {
            launcher,
            config_check,
            threads: Mutex::new(JoinSet::new()),
        })
    }

    pub fn config(&self) -> &Arc<LiveConfig> {
        &self.launcher.config
    }

    /// The model every thread talks to.
    pub fn model(&self) -> &Model {
        &self.launcher.model
    }

    pub fn paths(&self) -> &Paths {
        &self.launcher.paths
    }

    /// The tools a chat gets, by name.
    pub fn tool_names(&self) -> Vec<&'static str> {
        // Names do not depend on the thread.
        self.launcher.tools("", None, 0).names().collect()
    }

    /// Every thread, the most recently active first, for a client to offer.
    pub async fn list_threads(&self) -> Result<Vec<Thread>> {
        self.launcher.state.list_threads().await
    }

    /// The whole history of a thread, compacted messages included, oldest first.
    pub async fn thread_messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        self.launcher.state.list_messages(thread_id).await
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
        self.launcher
            .state
            .load_thread(thread_id)
            .await?
            .with_context(|| format!("thread {thread_id} does not exist"))?;
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
        for mailbox in self.launcher.mailboxes.lock().await.values() {
            mailbox.send(Input::Cancel);
        }
        let started = Instant::now();
        while !self.launcher.mailboxes.lock().await.is_empty()
            && started.elapsed() < AGENTS_STOP_WAIT
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.config_check.abort();
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
