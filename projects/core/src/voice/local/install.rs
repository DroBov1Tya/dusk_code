//! Making a model's environment: Python, its packages, and its weights.

use std::ffi::OsStr;

use anyhow::{Context, Result, ensure};

use super::{
    LocalVoice,
    catalog::{Model, SERVER_PACKAGES},
    uv,
};
use crate::{release::resources, secrets::HUGGINGFACE};

/// Written last: a directory without it is an install that did not finish.
pub(super) const READY: &str = "ready";

/// The Python the models are tested with; `uv` fetches it if the machine has none.
const PYTHON: &str = "3.12";

impl LocalVoice {
    /// Installs the model if it is not installed, then starts it and connects it.
    pub(super) async fn set_up(&self, model: &'static Model) -> Result<()> {
        if !self.installed(model) {
            self.make_environment(model).await?;
        }
        self.stage(model, "Starting the server");
        self.connect(model).await
    }

    async fn make_environment(&self, model: &Model) -> Result<()> {
        let uv = uv::find().await.context("uv is not installed")?;
        let dir = self.dir(model);
        tokio::fs::create_dir_all(&dir).await?;
        // Short on purpose: espeak, which Kokoro uses, cannot read data from a long path.
        let venv = dir.join("venv");
        let python = venv.join("bin/python");
        let weights = dir.join("hf");
        let script = resources::root()?.join("voice/server.py");
        ensure!(script.is_file(), "the speech server script is missing");

        self.stage(model, "Preparing Python");
        uv::run(
            &uv,
            &[
                "venv".as_ref(),
                "--clear".as_ref(),
                "--python".as_ref(),
                PYTHON.as_ref(),
                venv.as_os_str(),
            ],
            &[],
        )
        .await?;

        self.stage(model, "Installing packages");
        let mut args: Vec<&OsStr> = vec![
            "pip".as_ref(),
            "install".as_ref(),
            "--python".as_ref(),
            python.as_os_str(),
        ];
        // Without this, Linux fetches the CUDA build of torch, gigabytes the CPU never uses.
        if cfg!(target_os = "linux") && model.backend == "kokoro" {
            args.extend([OsStr::new("--torch-backend"), OsStr::new("cpu")]);
        }
        args.extend(
            SERVER_PACKAGES
                .iter()
                .chain(model.packages)
                .map(|package| OsStr::new(*package)),
        );
        uv::run(&uv, &args, &[]).await?;

        self.stage(model, &format!("Downloading the model, {}", model.size));
        let mut env = vec![
            ("HF_HOME", weights.display().to_string()),
            // The default downloader stalled in tests; plain HTTPS did not.
            ("HF_HUB_DISABLE_XET", "1".to_owned()),
            ("PYTHONNOUSERSITE", "1".to_owned()),
        ];
        env.extend(self.passthrough());
        if let Some(token) = self.hugging_face_token().await? {
            env.push(("HF_TOKEN", token));
        }
        uv::run(
            &python,
            &[
                script.as_os_str(),
                "download".as_ref(),
                "--backend".as_ref(),
                model.backend.as_ref(),
            ],
            &env,
        )
        .await?;

        tokio::fs::write(dir.join(READY), b"1").await?;
        Ok(())
    }

    /// What a download needs of the environment to reach the network and the disk.
    fn passthrough(&self) -> Vec<(&'static str, String)> {
        [
            "HOME",
            "PATH",
            "TMPDIR",
            "HTTPS_PROXY",
            "HTTP_PROXY",
            "NO_PROXY",
            "SSL_CERT_FILE",
        ]
        .into_iter()
        .filter_map(|name| Some((name, std::env::var(name).ok()?)))
        .collect()
    }

    async fn hugging_face_token(&self) -> Result<Option<String>> {
        let Some(store) = self.config.secrets().cloned() else {
            return Ok(None);
        };
        tokio::task::spawn_blocking(move || store.get(HUGGINGFACE))
            .await
            .context("the keychain call stopped")?
    }
}
