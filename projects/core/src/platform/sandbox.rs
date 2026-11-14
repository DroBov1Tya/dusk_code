//! Commands as the tools run them: `sh -c`, and in sandbox mode confined by the system so
//! they write only in the folders the agent works in and never read the config folder,
//! where the keys are. Programs of plugins run confined the same way, without a shell and
//! with tighter rules.

use std::path::{Path, PathBuf};

use anyhow::Result;
use tokio::process::Command;

/// Where a confined command may write, and what it may not touch at all.
#[derive(Debug, Clone)]
pub struct Confinement {
    pub writable: Vec<PathBuf>,
    pub hidden: Vec<PathBuf>,
    /// Folders inside a hidden one that stay open: read-only, or writable too when they
    /// are also in `writable`.
    pub reopened: Vec<PathBuf>,
    pub network: Network,
    /// The system keychain open. Only macOS can close it: on Linux it is a D-Bus service
    /// Landlock does not reach.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub keychain: bool,
    /// When set, besides the system's own programs only these files, or the programs
    /// under these folders, may be started.
    pub programs: Option<Vec<PathBuf>>,
}

/// What a confined program may reach over the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Open,
    /// No socket opened or bound at all.
    Closed,
    /// Only this port on this machine: the core's broker for the program.
    Broker(u16),
}

// Read by the Seatbelt profile; Landlock cannot keep a single port, so Linux does not.
#[cfg_attr(target_os = "linux", allow(dead_code))]
impl Network {
    pub fn is_open(self) -> bool {
        self == Self::Open
    }

    pub fn only_port(self) -> Option<u16> {
        match self {
            Self::Broker(port) => Some(port),
            Self::Open | Self::Closed => None,
        }
    }
}

impl Default for Confinement {
    /// Closed: nothing writable, no network, no keychain, only system programs.
    fn default() -> Self {
        Self {
            writable: Vec::new(),
            hidden: Vec::new(),
            reopened: Vec::new(),
            network: Network::Closed,
            keychain: false,
            programs: Some(Vec::new()),
        }
    }
}

impl Confinement {
    /// The working directory, the thread's workspace and the temporary folders open;
    /// the protected folder closed.
    pub fn around(cwd: &Path, workspace: &Path, protected: Option<&Path>) -> Self {
        let mut writable = vec![cwd.to_path_buf(), workspace.to_path_buf()];
        writable.extend(temp_folders());
        writable.dedup();
        Self {
            writable,
            hidden: protected.map(Path::to_path_buf).into_iter().collect(),
            reopened: Vec::new(),
            network: Network::Open,
            keychain: true,
            programs: None,
        }
    }
}

/// The system's temporary folders, resolved.
pub fn temp_folders() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = [std::env::temp_dir(), PathBuf::from("/tmp")]
        .into_iter()
        .filter_map(|folder| folder.canonicalize().ok())
        .collect();
    folders.dedup();
    folders
}

/// `sh -c command`, confined when `confinement` is given.
pub fn shell(command: &str, confinement: Option<&Confinement>) -> Result<Command> {
    let Some(confinement) = confinement else {
        let mut plain = Command::new("sh");
        plain.arg("-c").arg(command);
        return Ok(plain);
    };
    confined(
        Path::new("sh"),
        &["-c".to_owned(), command.to_owned()],
        confinement,
    )
}

/// `program` with `arguments`, no shell, confined. Fails when this system cannot
/// confine it, so nothing starts unconfined.
pub fn confined_program(
    program: &Path,
    arguments: &[String],
    confinement: &Confinement,
) -> Result<Command> {
    anyhow::ensure!(
        available(),
        "this system has no sandbox, so the program is not started"
    );
    confined(program, arguments, confinement)
}

#[cfg(target_os = "macos")]
fn confined(program: &Path, arguments: &[String], confinement: &Confinement) -> Result<Command> {
    super::macos::sandbox::command(program, arguments, confinement)
}

#[cfg(target_os = "linux")]
fn confined(program: &Path, arguments: &[String], confinement: &Confinement) -> Result<Command> {
    super::linux::sandbox::command(program, arguments, confinement)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn confined(_: &Path, _: &[String], _: &Confinement) -> Result<Command> {
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
