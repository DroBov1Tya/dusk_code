//! Commands as the tools run them: `sh -c`, and in sandbox mode confined by the system so
//! they write only in the folders the agent works in and never read the config folder,
//! where the keys are.

use std::path::{Path, PathBuf};

use anyhow::Result;
use tokio::process::Command;

/// Where a confined command may write, and what it may not touch at all.
#[derive(Debug, Clone, Default)]
pub struct Confinement {
    pub writable: Vec<PathBuf>,
    pub hidden: Vec<PathBuf>,
}

impl Confinement {
    /// The working directory, the thread's workspace and the temporary folders open;
    /// the protected folder closed.
    pub fn around(cwd: &Path, workspace: &Path, protected: Option<&Path>) -> Self {
        let mut writable = vec![cwd.to_path_buf(), workspace.to_path_buf()];
        writable.extend(
            [std::env::temp_dir(), PathBuf::from("/tmp")]
                .into_iter()
                .filter_map(|folder| folder.canonicalize().ok()),
        );
        writable.dedup();
        Self {
            writable,
            hidden: protected.map(Path::to_path_buf).into_iter().collect(),
        }
    }
}

/// `sh -c command`, confined when `confinement` is given.
pub fn shell(command: &str, confinement: Option<&Confinement>) -> Result<Command> {
    let Some(confinement) = confinement else {
        let mut plain = Command::new("sh");
        plain.arg("-c").arg(command);
        return Ok(plain);
    };
    confined(command, confinement)
}

#[cfg(target_os = "macos")]
fn confined(command: &str, confinement: &Confinement) -> Result<Command> {
    super::macos::sandbox::command(command, confinement)
}

#[cfg(target_os = "linux")]
fn confined(command: &str, confinement: &Confinement) -> Result<Command> {
    super::linux::sandbox::command(command, confinement)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn confined(_: &str, _: &Confinement) -> Result<Command> {
    anyhow::bail!("this system has no sandbox for commands; switch to ask or full access")
}

pub fn available() -> bool {
    #[cfg(target_os = "macos")]
    return super::macos::sandbox::available();
    #[cfg(target_os = "linux")]
    return super::linux::sandbox::available();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    false
}
