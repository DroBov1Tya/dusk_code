//! `uv`, which makes the Python environments of the models, and running it.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use tokio::{io::AsyncReadExt, process::Command};

use crate::platform::login_path;

/// A whole install may be slow on a poor connection; this only stops one that hangs.
const STEP_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// How much of a failed step's output is kept to say why.
const TAIL: usize = 600;

/// Where `uv` usually lives when the app was started without the user's shell PATH.
const COMMON: [&str; 4] = [
    ".local/bin",
    ".cargo/bin",
    "/opt/homebrew/bin",
    "/usr/local/bin",
];

/// `uv` in the user's PATH or where its installers put it.
pub async fn find() -> Option<PathBuf> {
    login_path::load().await;
    let mut dirs: Vec<PathBuf> = login_path::get()
        .map(|path| std::env::split_paths(path).collect())
        .unwrap_or_default();
    dirs.extend(
        std::env::var_os("PATH")
            .iter()
            .flat_map(std::env::split_paths),
    );
    let home = std::env::var_os("HOME").map(PathBuf::from);
    for common in COMMON {
        let path = Path::new(common);
        match (&home, path.is_absolute()) {
            (_, true) => dirs.push(path.to_owned()),
            (Some(home), false) => dirs.push(home.join(path)),
            _ => {}
        }
    }
    dirs.into_iter()
        .map(|dir| dir.join("uv"))
        .find(|candidate| candidate.is_file())
}

/// Runs a program to its end and keeps what it said if it failed.
pub async fn run(program: &Path, args: &[&std::ffi::OsStr], env: &[(&str, String)]) -> Result<()> {
    let mut command = Command::new(program);
    command
        .args(args)
        .envs(env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .with_context(|| format!("cannot start {}", program.display()))?;
    let mut stderr = child.stderr.take().context("no output to read")?;
    let work = async {
        let mut said = Vec::new();
        let (_, status) = tokio::join!(stderr.read_to_end(&mut said), child.wait());
        (said, status)
    };
    let Ok((said, status)) = tokio::time::timeout(STEP_TIMEOUT, work).await else {
        bail!("{} took too long", program.display());
    };
    if status?.success() {
        return Ok(());
    }
    bail!("{}", tail(&said))
}

/// The end of what a program said, as text.
pub fn tail(said: &[u8]) -> String {
    let text = String::from_utf8_lossy(said);
    let text = text.trim();
    let start = text
        .char_indices()
        .rev()
        .nth(TAIL)
        .map_or(0, |(index, _)| index);
    text[start..].to_owned()
}
