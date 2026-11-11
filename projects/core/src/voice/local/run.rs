//! A model's server: started on a loopback port with a token made for this run, and the
//! speech settings pointed at it.

use std::{process::Stdio, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::config::{STT_KEY, SpeechKind, TTS_KEY, VoiceEndpoint};
use parking_lot::Mutex;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, Command},
};

use super::{LocalVoice, catalog::Model, uv};
use crate::release::resources;

/// Loading a model off the disk takes a while the first time.
const READY_WAIT: Duration = Duration::from_secs(180);

pub struct Running {
    child: Child,
    port: u16,
    token: String,
}

impl Running {
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

fn key_of(kind: SpeechKind) -> &'static str {
    match kind {
        SpeechKind::Recognition => STT_KEY,
        SpeechKind::Synthesis => TTS_KEY,
    }
}

impl LocalVoice {
    /// Starts the server if it is not running, and points the speech settings at it.
    pub(super) async fn connect(&self, model: &'static Model) -> Result<()> {
        let (port, token) = self.start(model).await?;
        let key = key_of(model.kind);
        if let Some(store) = self.config.secrets().cloned() {
            tokio::task::spawn_blocking(move || store.set(key, &token))
                .await
                .context("the keychain call stopped")??;
        }
        let endpoint = VoiceEndpoint {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            model: model.id.to_owned(),
            voice: model.voice.map(str::to_owned),
        };
        self.config.update(|config| {
            let mut voice = config.voice().clone();
            match model.kind {
                SpeechKind::Recognition => voice.stt = Some(endpoint),
                SpeechKind::Synthesis => voice.tts = Some(endpoint),
            }
            config.set_voice(voice);
        })?;
        self.choose(model)
    }

    /// Takes the model out of the speech settings, if they point at it.
    pub(super) fn disconnect(&self, model: &Model) -> Result<()> {
        let mut was_used = false;
        self.config.update(|config| {
            let mut voice = config.voice().clone();
            let slot = match model.kind {
                SpeechKind::Recognition => &mut voice.stt,
                SpeechKind::Synthesis => &mut voice.tts,
            };
            if slot
                .as_ref()
                .is_some_and(|endpoint| endpoint.model == model.id)
            {
                *slot = None;
                was_used = true;
                config.set_voice(voice);
            }
        })?;
        if was_used && let Some(store) = self.config.secrets() {
            store.delete(key_of(model.kind))?;
        }
        self.forget(model)
    }

    pub(super) async fn stop(&self, model: &Model) {
        if let Some(mut running) = self.running.lock().await.remove(model.id) {
            let _ = running.child.kill().await;
        }
    }

    /// The port and the token of the running server; it is started first if it is not.
    async fn start(&self, model: &'static Model) -> Result<(u16, String)> {
        if let Some(running) = self.running.lock().await.get_mut(model.id)
            && running.alive()
        {
            return Ok((running.port, running.token.clone()));
        }
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))?
            .local_addr()?
            .port();
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let dir = self.dir(model);
        let python = dir.join("venv/bin/python");
        let script = resources::root()?.join("voice/server.py");
        ensure!(python.is_file(), "{} is not installed", model.title);
        ensure!(script.is_file(), "the speech server script is missing");
        let mut command = Command::new(&python);
        command
            .arg(&script)
            .args(["serve", "--backend", model.backend, "--host", "127.0.0.1"])
            .arg("--port")
            .arg(port.to_string())
            // The server gets only what it needs; none of the app's environment.
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", std::env::var_os("HOME").unwrap_or_default())
            .env("HF_HOME", dir.join("hf"))
            // Everything was downloaded when it was installed.
            .env("HF_HUB_OFFLINE", "1")
            .env("PYTHONNOUSERSITE", "1")
            .env("DUSK_VOICE_TOKEN", &token)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().context("cannot start the speech server")?;
        let said = Arc::new(Mutex::new(Vec::<u8>::new()));
        drain_stderr(child.stderr.take(), Arc::clone(&said));
        let stdout = child.stdout.take().context("the server has no output")?;
        let mut lines = BufReader::new(stdout).lines();
        let ready = tokio::time::timeout(READY_WAIT, async {
            while let Some(line) = lines.next_line().await? {
                if is_ready(&line) {
                    return anyhow::Ok(true);
                }
            }
            Ok(false)
        })
        .await;
        if !matches!(ready, Ok(Ok(true))) {
            let why = uv::tail(&said.lock());
            bail!("the speech server did not start: {why}");
        }
        // Whatever it prints later is read and dropped, so its pipe never fills.
        tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
        self.running.lock().await.insert(
            model.id,
            Running {
                child,
                port,
                token: token.clone(),
            },
        );
        Ok((port, token))
    }
}

pub(super) fn is_ready(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .is_some_and(|value| value["stage"] == "ready")
}

/// Keeps the end of what the server says on its error output, to explain a failure.
fn drain_stderr(stderr: Option<tokio::process::ChildStderr>, said: Arc<Mutex<Vec<u8>>>) {
    let Some(mut stderr) = stderr else { return };
    tokio::spawn(async move {
        let mut chunk = [0u8; 4096];
        while let Ok(read) = stderr.read(&mut chunk).await {
            if read == 0 {
                break;
            }
            let mut said = said.lock();
            said.extend_from_slice(&chunk[..read]);
            let excess = said.len().saturating_sub(8192);
            said.drain(..excess);
        }
    });
}
