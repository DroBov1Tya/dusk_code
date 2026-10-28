//! The core's runtime: what a client starts once and asks for threads. It owns the
//! database, the live config and the model, checks the config file in the background
//! and gives each active thread its own task.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use tokio::task::{JoinHandle, JoinSet};
use tracing::info;

use super::thread_runner::{ThreadInput, ThreadOutput, ThreadRunner, ThreadSetup};
use crate::{
    init::{self, create_private_dir},
    live_config::LiveConfig,
    paths::Paths,
    providers::{Model, ModelProvider, Provider, ProviderRegistry},
    state::{Message, State, Thread},
    thread_start::start_thread,
    tools::{ReadHistory, ToolRegistry, ToolScope},
};

/// Hand edits to config.yml are picked up within this time.
const CONFIG_CHECK_PERIOD: Duration = Duration::from_secs(60);

pub struct Runtime {
    paths: Paths,
    state: Arc<State>,
    config: Arc<LiveConfig>,
    provider: Arc<Provider>,
    model: Model,
    config_check: JoinHandle<()>,
    threads: Mutex<JoinSet<()>>,
    /// Threads with a running task.
    active: Arc<Mutex<HashSet<String>>>,
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
        Ok(Self {
            paths,
            state: Arc::new(state),
            config,
            provider,
            model,
            config_check,
            threads: Mutex::new(JoinSet::new()),
            active: Arc::default(),
        })
    }

    pub fn config(&self) -> &Arc<LiveConfig> {
        &self.config
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The tools every thread gets, by name.
    pub fn tool_names(&self) -> Vec<&'static str> {
        // Names do not depend on the thread.
        self.tools("").names().collect()
    }

    /// Every thread, the most recently active first, for a client to offer.
    pub async fn list_threads(&self) -> Result<Vec<Thread>> {
        self.state.list_threads().await
    }

    /// The whole history of a thread, compacted messages included, oldest first.
    pub async fn thread_messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        self.state.list_messages(thread_id).await
    }

    /// Starts a thread whose tools work in `cwd`, and the task that runs it. The task
    /// ends once the client drops every `ThreadInput` and the queue is done.
    pub async fn new_thread(&self, source: &str, cwd: &Path) -> Result<ThreadHandle> {
        let thread = start_thread(&self.state, &self.paths, source).await?;
        info!(
            "thread {} started in {}",
            thread.thread_id,
            thread.workspace.display()
        );
        self.run_thread(thread.thread_id, &thread.workspace, cwd)
    }

    /// Picks a stored thread up again, with its tools working in `cwd`. What it left
    /// unfinished is closed first: a running turn ends as interrupted, and messages sent
    /// before a crash are taken in.
    pub async fn resume_thread(&self, thread_id: &str, cwd: &Path) -> Result<ThreadHandle> {
        self.state
            .load_thread(thread_id)
            .await?
            .with_context(|| format!("thread {thread_id} does not exist"))?;
        let workspace = self.paths.thread_workspace(thread_id)?;
        create_private_dir(&workspace)?;
        info!("thread {thread_id} resumed in {}", workspace.display());
        self.run_thread(thread_id.to_owned(), &workspace, cwd)
    }

    /// One task per thread: two would answer the same messages twice.
    fn run_thread(&self, thread_id: String, workspace: &Path, cwd: &Path) -> Result<ThreadHandle> {
        let (runner, input, output) = ThreadRunner::new(ThreadSetup {
            state: Arc::clone(&self.state),
            provider: Arc::clone(&self.provider) as Arc<dyn ModelProvider>,
            model: self.model.clone(),
            config: Arc::clone(&self.config),
            thread_id: thread_id.clone(),
            tools: self.tools(&thread_id),
            scope: ToolScope::new(cwd, workspace)?,
            compact_prompt: self.paths.compact_prompt(),
        });
        let active = Arc::clone(&self.active);
        ensure!(
            lock(&active).insert(thread_id.clone()),
            "thread {thread_id} is already open"
        );
        let id = thread_id.clone();
        self.lock_threads().spawn(async move {
            runner.run().await;
            lock(&active).remove(&id);
        });
        Ok(ThreadHandle {
            thread_id,
            input,
            output,
        })
    }

    /// Lets every thread finish its queue, stops the config check and closes the database.
    pub async fn shutdown(self) {
        let mut threads = std::mem::take(&mut *self.lock_threads());
        while threads.join_next().await.is_some() {}
        self.config_check.abort();
        self.state.close().await;
    }

    /// The built-in tools plus the thread's own history.
    fn tools(&self, thread_id: &str) -> ToolRegistry {
        let mut tools = ToolRegistry::builtin(&self.paths.skills_dir());
        let history = ReadHistory::new(Arc::clone(&self.state), thread_id.to_owned());
        tools.add(Box::new(history));
        tools
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
