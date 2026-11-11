//! The core's runtime: what a client starts once and asks for threads. It owns the
//! database, the live config and the model, checks the config file in the background
//! and gives each active thread its own task.

use super::{
    Launcher, ThreadInput, ThreadOutput,
    model_choice::{Choice, ModelChoice},
};
use crate::platform::Platform;
use crate::runtime::ThreadEvent;
use crate::runtime::agent::{Input, Mailboxes};
use crate::runtime::jobs::JobManager;
use crate::{
    catalog::agents::Agents, config::live::LiveConfig, init, init::paths::Paths, mcp::McpServers,
    release::watch::UpdateWatch, state::State,
};
use anyhow::Result;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;
use tracing::{info, warn};

/// Hand edits to config.yml are picked up within this time.
const CONFIG_CHECK_PERIOD: Duration = Duration::from_secs(60);

/// How long shutdown waits for stopped subagents to end.
const AGENTS_STOP_WAIT: Duration = Duration::from_secs(5);

pub struct Runtime {
    pub(super) launcher: Launcher,
    pub(super) config_check: JoinHandle<()>,
    pub(super) agents_check: JoinHandle<()>,
    pub(super) plugins: Arc<crate::plugins::Plugins>,
    pub(super) plugins_check: JoinHandle<()>,
    pub(super) platforms: Arc<crate::plugins::Platforms>,
    pub(super) update_check: JoinHandle<()>,
    pub(super) updates: Arc<UpdateWatch>,
    pub(super) model_discovery: Option<JoinHandle<()>>,
    /// Shipped files the user changed, kept at the start, with the new version beside.
    pub(super) updated_builtin: Vec<String>,
    /// The latest sounds made by reading aloud, by what was asked, so asking again plays the
    /// same card and the artifact folder does not fill; the oldest goes when it is full.
    pub(super) speech:
        parking_lot::Mutex<std::collections::VecDeque<(String, dusk_protocol::tools::Card)>>,
    /// Speech models installed on this machine, and the servers that run them.
    pub(super) local_voice: Arc<crate::voice::LocalVoice>,
    pub(super) state_db: PathBuf,
    // Declared last so coordination is released after the other owned runtime fields.
    pub(super) _restore_guard: Option<crate::backup::archive::RestoreGuard>,
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
        let state_db = crate::backup::archive::resolve_state_db(
            &state_db.unwrap_or_else(|| paths.state_db()),
        )?;
        let restore_guard = crate::backup::archive::RestoreGuard::startup(&paths, Some(&state_db))?;
        // A client that shows the restore calls this first; then it is not tried again.
        let version = crate::release::update::VERSION;
        match crate::release::complete::ensure_installed(&|_| {}).await {
            Ok(None) => {}
            Ok(Some(fetched)) => {
                info!("resources for {version} restored from GitHub, {fetched} files")
            }
            Err(error) => warn!("cannot restore the resources for {version}: {error:#}"),
        }
        let initialized = init::initialize(&paths)?;
        let state = Arc::new(State::open(&state_db).await?);
        let mailboxes = Arc::new(Mailboxes::default());
        let jobs = JobManager::new(Arc::clone(&state), Arc::clone(&mailboxes), &state_db).await?;
        let config = Arc::new(LiveConfig::load_with(
            paths.config_file(),
            crate::secrets::Keychain::open(paths.config_root()),
        )?);
        let choice = match Choice::configured(&config.current()) {
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
        let local_voice = Arc::new(crate::voice::LocalVoice::new(
            paths.voice_root(),
            Arc::clone(&config),
        ));
        tokio::spawn({
            let local_voice = Arc::clone(&local_voice);
            async move { local_voice.autostart().await }
        });
        let config_check = tokio::spawn(Arc::clone(&config).watch(CONFIG_CHECK_PERIOD));
        let agents = Agents::new(paths.agents_dir(), Arc::clone(&state));
        if let Err(error) = agents.scan().await {
            warn!("cannot read the agents: {error:#}");
        }
        let agents_check = tokio::spawn(Arc::clone(&agents).watch(CONFIG_CHECK_PERIOD));
        let updates = Arc::new(UpdateWatch::default());
        let update_check = tokio::spawn(Arc::clone(&updates).run(Arc::clone(&config)));
        let mcp = McpServers::start(&config.current(), paths.mcp_tokens_dir());
        let platform = Platform::detect();
        info!(
            "platform: {:?}, screen: {}, sandbox: {}",
            platform.os, platform.gui, platform.sandbox
        );
        let hooks = Arc::new(crate::plugins::Hooks::default());
        let platforms = Arc::new(crate::plugins::Platforms::default());
        let launcher = Launcher {
            threads: Arc::default(),
            hooks: Arc::clone(&hooks),
            state,
            model: Arc::new(choice.map_or_else(ModelChoice::offline, ModelChoice::new)),
            config: Arc::clone(&config),
            paths,
            mailboxes,
            jobs,
            approvals: Arc::default(),
            platform,
            mcp,
            agents,
        };
        launcher
            .approvals
            .set_judge(Arc::new(super::arbiter::Arbiter::new(
                Arc::clone(&launcher.state),
                Arc::clone(&launcher.model),
            )));
        platforms.attach(launcher.clone());
        // Read before model discovery, so a plugin's provider can be the first that answers.
        let plugins = crate::plugins::Plugins::new(
            launcher.paths.clone(),
            Arc::clone(&config),
            Arc::clone(&launcher.mcp),
            hooks,
            Arc::clone(&platforms),
        );
        plugins.scan();
        let plugins_check = tokio::spawn(Arc::clone(&plugins).watch(CONFIG_CHECK_PERIOD));
        let model_discovery = if !config.current().all_providers().is_empty() {
            let config = Arc::clone(&config);
            let model = Arc::clone(&launcher.model);
            let mailboxes = Arc::clone(&launcher.mailboxes);
            Some(tokio::spawn(async move {
                let revision = model.revision();
                let selected = if let Some(saved) = model.current() {
                    saved
                        .refresh()
                        .await
                        .map(|metadata| model.refresh_model(&saved, metadata))
                } else {
                    Choice::pick(&config.current(), None).await.map(|choice| {
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
                                input_limit: selected.input_limit(&config.current()),
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
            _restore_guard: restore_guard,
            launcher,
            config_check,
            agents_check,
            plugins,
            plugins_check,
            platforms,
            update_check,
            updates,
            model_discovery,
            updated_builtin: initialized.builtin.updated,
            speech: Default::default(),
            local_voice,
            state_db,
        })
    }

    pub fn config(&self) -> &Arc<LiveConfig> {
        &self.launcher.config
    }

    /// Lets every chat finish its queue, stops the agents still at work, stops the
    /// config check and closes the database.
    pub async fn shutdown(self) {
        // A platform would keep its thread open, so it goes before the threads end.
        self.platforms.stop_all();
        let mut threads = std::mem::take(&mut *self.launcher.threads.lock());
        while threads.join_next().await.is_some() {}
        // No agent starts from here on; those at work are stopped until none is left,
        // since one may have started a moment before the stop.
        let mailboxes = &self.launcher.mailboxes;
        mailboxes.close();
        self.launcher.jobs.shutdown().await;
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
        self.plugins_check.abort();
        self.update_check.abort();
        if let Some(discovery) = self.model_discovery {
            discovery.abort();
        }
        self.launcher.state.close().await;
    }
}
