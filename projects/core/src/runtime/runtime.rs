//! The core's runtime: what a client starts once and asks for threads. It owns the
//! database, the live config and the model, checks the config file in the background
//! and gives each active thread its own task.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use tokio::task::{JoinHandle, JoinSet};
use tracing::info;

use super::thread_runner::{ThreadInput, ThreadOutput, ThreadRunner, ThreadSetup};
use crate::{
    init,
    live_config::LiveConfig,
    paths::Paths,
    providers::{Model, ModelProvider, Provider, ProviderRegistry},
    state::State,
    thread_start::start_thread,
    tools::{ToolRegistry, ToolScope},
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
}

/// A started thread, as its client sees it.
pub struct ThreadHandle {
    pub thread_id: String,
    /// Where the agent keeps temporary and downloaded files for this thread.
    pub workspace: PathBuf,
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
        })
    }

    pub fn config(&self) -> &Arc<LiveConfig> {
        &self.config
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// The tools every thread gets, by name.
    pub fn tool_names(&self) -> Vec<&'static str> {
        self.tools().names().collect()
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
        let (runner, input, output) = ThreadRunner::new(ThreadSetup {
            state: Arc::clone(&self.state),
            provider: Arc::clone(&self.provider) as Arc<dyn ModelProvider>,
            model: self.model.clone(),
            config: Arc::clone(&self.config),
            thread_id: thread.thread_id.clone(),
            tools: self.tools(),
            scope: ToolScope::new(cwd, &thread.workspace)?,
        });
        self.lock_threads().spawn(runner.run());
        Ok(ThreadHandle {
            thread_id: thread.thread_id,
            workspace: thread.workspace,
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

    fn tools(&self) -> ToolRegistry {
        ToolRegistry::builtin(&self.paths.skills_dir())
    }

    fn lock_threads(&self) -> std::sync::MutexGuard<'_, JoinSet<()>> {
        // Spawning and taking the set cannot leave it half-changed.
        self.threads.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
