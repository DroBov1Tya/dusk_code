//! Fixed installers for the local desktop's phone panels. No plugin is given these
//! commands or their output. Each install begins from the desktop's own button.

#[cfg(target_os = "macos")]
mod launchpad;

#[cfg(target_os = "macos")]
use std::path::PathBuf;

use anyhow::Result;

#[cfg(target_os = "macos")]
pub(crate) const AXE_PATHS: [&str; 2] = ["/opt/homebrew/bin/axe", "/usr/local/bin/axe"];
#[cfg(target_os = "macos")]
pub(crate) const APP_NAME: &str = "vphone-launchpad.app";
#[cfg(target_os = "macos")]
pub(crate) const CLI_SUBPATH: &str = "Contents/MacOS/vphone-launchpad-cli";

#[cfg(target_os = "macos")]
pub(crate) fn user_app() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    if !home.is_absolute() {
        return None;
    }
    Some(
        home.canonicalize()
            .ok()?
            .join("Applications")
            .join(APP_NAME),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhoneDependency {
    Axe,
    Launchpad,
}

impl PhoneDependency {
    pub fn for_panel(plugin: &str, part: &str) -> Option<Self> {
        match (plugin, part) {
            ("ios-simulator", "phone") => Some(Self::Axe),
            ("vphone", "phone") => Some(Self::Launchpad),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Axe => "Install AXe",
            Self::Launchpad => "Install vphone Launchpad",
        }
    }

    pub fn progress(self) -> &'static str {
        match self {
            Self::Axe => "Installing AXe with Homebrew",
            Self::Launchpad => "Installing vphone Launchpad",
        }
    }

    pub async fn install(self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            static ACTIVE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
            let _guard = ACTIVE
                .try_lock()
                .context("A phone dependency is already installing")?;
            match self {
                Self::Axe => install_axe().await,
                Self::Launchpad => launchpad::install().await,
            }
        }
        #[cfg(not(target_os = "macos"))]
        anyhow::bail!("Phone dependencies can only be installed on macOS");
    }
}

#[cfg(target_os = "macos")]
use {
    anyhow::{Context, bail, ensure},
    std::{path::Path, process::Stdio, time::Duration},
    tokio::io::{AsyncRead, AsyncReadExt},
};

#[cfg(target_os = "macos")]
async fn install_axe() -> Result<()> {
    let brew = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
        .context("Install Homebrew from brew.sh, then try again.")?;
    let prefix = Path::new(brew)
        .parent()
        .context("Could not locate Homebrew")?;
    let axe = prefix.join("axe");
    run(
        brew,
        &["install", "cameroncooke/axe/axe"],
        Duration::from_secs(600),
        65536,
    )
    .await
    .context("Homebrew could not install AXe")?;
    run(
        axe.to_str().context("Could not locate AXe")?,
        &["--version"],
        Duration::from_secs(30),
        4096,
    )
    .await
    .context("Homebrew completed, but AXe did not start")?;
    Ok(())
}

#[cfg(target_os = "macos")]
async fn run(program: &str, args: &[&str], limit: Duration, max: usize) -> Result<Vec<u8>> {
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .env_clear()
        .envs(std::env::vars_os().filter(|(key, _)| {
            matches!(
                key.to_str(),
                Some("HOME" | "USER" | "LOGNAME" | "TMPDIR" | "LANG" | "PATH")
            )
        }))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("{program} did not start"))?;
    let out = child.stdout.take().context("No standard output")?;
    let err = child.stderr.take().context("No standard error")?;
    let (out, err, status) = tokio::time::timeout(limit, async {
        tokio::try_join!(drain(out, max), drain(err, 65536), child.wait())
    })
    .await
    .with_context(|| format!("{program} took too long"))??;
    if !status.success() {
        let detail: String = String::from_utf8_lossy(&err)
            .lines()
            .rfind(|line| !line.trim().is_empty())
            .unwrap_or("The process failed")
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(300)
            .collect();
        bail!("{detail}");
    }
    ensure!(out.len() <= max, "{program} printed too much");
    Ok(out)
}

#[cfg(target_os = "macos")]
async fn drain(mut from: impl AsyncRead + Unpin, max: usize) -> std::io::Result<Vec<u8>> {
    let mut kept = Vec::new();
    (&mut from)
        .take(max as u64 + 1)
        .read_to_end(&mut kept)
        .await?;
    tokio::io::copy(&mut from, &mut tokio::io::sink()).await?;
    Ok(kept)
}
