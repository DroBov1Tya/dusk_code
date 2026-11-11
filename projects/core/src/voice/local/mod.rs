//! Speech models that run on this machine. `uv` makes an environment for a model under the
//! app's own data directory, installs its packages and has its weights downloaded there;
//! `resources/voice/server.py` then serves it on a loopback port, with a token made for
//! each run, and the chat's speech settings are pointed at that server. What was chosen
//! starts again with the app. Nothing here is a model's own code: removing a model deletes
//! its directory and nothing else.

mod catalog;
mod install;
mod run;
mod state;
#[cfg(test)]
mod tests;
mod uv;

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::{Result, bail, ensure};
use dusk_protocol::config::{LocalState, VoiceModel, VoiceModels, VoiceSettings};
use parking_lot::Mutex;

use crate::config::live::LiveConfig;
use catalog::{MODELS, Model, find};
use run::Running;

enum Job {
    Installing(String),
    Failed(String),
}

pub struct LocalVoice {
    /// `<data>/voice`: a directory for each model, and what is chosen.
    root: PathBuf,
    config: Arc<LiveConfig>,
    jobs: Mutex<HashMap<&'static str, Job>>,
    /// What stops an install that is under way.
    tasks: Mutex<HashMap<&'static str, tokio::task::AbortHandle>>,
    running: tokio::sync::Mutex<HashMap<&'static str, Running>>,
}

impl LocalVoice {
    pub fn new(root: PathBuf, config: Arc<LiveConfig>) -> Self {
        Self {
            root,
            config,
            jobs: Mutex::default(),
            tasks: Mutex::default(),
            running: Default::default(),
        }
    }

    /// The models and where each is; `voice` says which the chat uses.
    pub async fn list(&self, voice: &VoiceSettings) -> VoiceModels {
        let mut running = self.running.lock().await;
        let mut models = Vec::new();
        for model in MODELS {
            let alive = running.get_mut(model.id).is_some_and(Running::alive);
            if !alive {
                running.remove(model.id);
            }
            let state = match self.jobs.lock().get(model.id) {
                Some(Job::Installing(stage)) => LocalState::Installing(stage.clone()),
                Some(Job::Failed(why)) => LocalState::Failed(why.clone()),
                None if alive => LocalState::Running,
                None if self.installed(model) => LocalState::Installed,
                None => LocalState::NotInstalled,
            };
            let used = match model.kind {
                dusk_protocol::config::SpeechKind::Recognition => voice.stt.as_ref(),
                dusk_protocol::config::SpeechKind::Synthesis => voice.tts.as_ref(),
            };
            models.push(VoiceModel {
                id: model.id.to_owned(),
                title: model.title.to_owned(),
                about: model.about.to_owned(),
                kind: model.kind,
                size: model.size.to_owned(),
                languages: model.languages.to_owned(),
                state,
                in_use: used.is_some_and(|endpoint| endpoint.model == model.id),
            });
        }
        VoiceModels {
            uv_found: uv::find().await.is_some(),
            models,
        }
    }

    /// Installs a model that is not here yet, then starts it and points the speech settings
    /// at it. Returns at once; `list` shows how far it got.
    pub async fn install(self: &Arc<Self>, id: &str) -> Result<()> {
        let model = find(id).ok_or_else(|| anyhow::anyhow!("no speech model {id}"))?;
        ensure!(
            self.installed(model) || uv::find().await.is_some(),
            "uv is not installed. It sets the models up for you: see docs.astral.sh/uv, then \
             try again"
        );
        {
            let mut jobs = self.jobs.lock();
            ensure!(
                !matches!(jobs.get(model.id), Some(Job::Installing(_))),
                "{} is being installed",
                model.title
            );
            jobs.insert(model.id, Job::Installing("Starting".into()));
        }
        let this = Arc::clone(self);
        let task = tokio::spawn(async move {
            let done = this.set_up(model).await;
            this.tasks.lock().remove(model.id);
            let mut jobs = this.jobs.lock();
            match done {
                Ok(()) => jobs.remove(model.id),
                Err(error) => {
                    tracing::warn!("speech model {}: {error:#}", model.id);
                    jobs.insert(model.id, Job::Failed(format!("{error:#}")))
                }
            };
        });
        self.tasks.lock().insert(model.id, task.abort_handle());
        Ok(())
    }

    /// Stops an install that is under way. Its programs stop with it, and what it had made
    /// is deleted unless the model was installed before.
    pub async fn cancel(&self, id: &str) -> Result<()> {
        let model = find(id).ok_or_else(|| anyhow::anyhow!("no speech model {id}"))?;
        if let Some(task) = self.tasks.lock().remove(model.id) {
            task.abort();
        }
        self.jobs.lock().remove(model.id);
        if !self.installed(model) {
            let dir = self.dir(model);
            if dir.exists() {
                tokio::fs::remove_dir_all(&dir).await?;
            }
        }
        Ok(())
    }

    /// Stops a model and deletes its directory.
    pub async fn remove(&self, id: &str) -> Result<()> {
        let model = find(id).ok_or_else(|| anyhow::anyhow!("no speech model {id}"))?;
        if matches!(self.jobs.lock().get(model.id), Some(Job::Installing(_))) {
            bail!("{} is being installed; wait for it to end", model.title);
        }
        self.stop(model).await;
        self.disconnect(model)?;
        let dir = self.dir(model);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await?;
        }
        self.jobs.lock().remove(model.id);
        Ok(())
    }

    /// Starts again what was chosen before the app closed.
    pub async fn autostart(self: &Arc<Self>) {
        for id in self.chosen() {
            let Some(model) = find(&id).filter(|model| self.installed(model)) else {
                continue;
            };
            if let Err(error) = self.connect(model).await {
                tracing::warn!("speech model {}: {error:#}", model.id);
                self.jobs
                    .lock()
                    .insert(model.id, Job::Failed(format!("{error:#}")));
            }
        }
    }

    fn dir(&self, model: &Model) -> PathBuf {
        self.root.join(model.id)
    }

    fn installed(&self, model: &Model) -> bool {
        self.dir(model).join(install::READY).is_file()
    }

    fn stage(&self, model: &Model, text: &str) {
        self.jobs
            .lock()
            .insert(model.id, Job::Installing(text.to_owned()));
    }
}
