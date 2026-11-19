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
    /// Folders whose Unix sockets it may connect to while the network is closed. Only
    /// macOS keeps to them; on Linux Landlock does not reach Unix sockets.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub sockets: Vec<PathBuf>,
    /// May hand work to the system to run outside this confinement (macOS: launchd's
    /// `launchctl`, LaunchServices' `open`, Apple events). Only macOS can close it.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub hand_off: bool,
    /// Folders whose own entry, not what is in them, may be looked at inside a hidden
    /// one: the way down to a reopened folder that a program resolves step by step, as
    /// git does its git dir. Landlock never closes that, so only macOS needs it.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub stat: Vec<PathBuf>,
    /// Folders whose list of entries, not the files in them, may be read inside a hidden
    /// one: the top of a repository git moves to and asks its own path. Every name there
    /// shows, those of entries git does not track or ignores too. Only macOS needs it:
    /// git on Linux learns its path from the kernel, and Landlock keeps the list closed.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub listed: Vec<PathBuf>,
    /// Linux: every file right the running Landlock knows is handled (truncation and
    /// device ioctls among them), and a Landlock too old to handle truncation refuses.
    /// macOS closes them all anyway.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub every_file_right: bool,
}

/// What a confined program may reach over the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Open,
    /// No socket opened or bound at all.
    Closed,
    /// Only this port on this machine: the core's broker for the program.
    Broker(u16),
    /// Linux: a network namespace of its own, whose only socket is the broker's, made
    /// by `namespaced_program`. Never built on other systems.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Namespace,
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
            Self::Open | Self::Closed | Self::Namespace => None,
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
            sockets: Vec::new(),
            hand_off: false,
            stat: Vec::new(),
            listed: Vec::new(),
            every_file_right: false,
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
            sockets: Vec::new(),
            hand_off: true,
            stat: Vec::new(),
            listed: Vec::new(),
            every_file_right: false,
        }
    }
}

/// The developer tools `xcrun` starts: Xcode's chosen folder and the private frameworks
/// its tools run from. Empty where there are none. Asked once: every plugin scan needs
/// it, and Xcode's choice seldom changes while the core runs.
pub fn developer_tools() -> Vec<PathBuf> {
    static FOLDERS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    FOLDERS.get_or_init(find_developer_tools).clone()
}

fn find_developer_tools() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let mut folders = Vec::new();
        if let Ok(output) = std::process::Command::new("/usr/bin/xcode-select")
            .arg("-p")
            .output()
            && output.status.success()
        {
            let chosen = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            folders.extend(Path::new(&chosen).canonicalize().ok());
        }
        folders.extend(
            Path::new("/Library/Developer/PrivateFrameworks")
                .canonicalize()
                .ok(),
        );
        folders
    }
    #[cfg(not(target_os = "macos"))]
    Vec::new()
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

/// `program` with `arguments`, no shell, confined, in a user and network namespace of
/// its own whose only socket is the one the receiver hands over once it has started:
/// the broker's. It may open no Unix or vsock socket. Fails where this Linux cannot
/// make one.
#[cfg(target_os = "linux")]
pub fn namespaced_program(
    program: &Path,
    arguments: &[String],
    confinement: &Confinement,
) -> Result<(Command, super::linux::netns::Receiver)> {
    anyhow::ensure!(
        available() && confinement.network == Network::Namespace,
        "this system cannot confine the program in a network of its own"
    );
    let mut command = Command::new(program);
    command.args(arguments);
    // First: Landlock, applied after it, would close /proc to the namespace's maps.
    let receiver = super::linux::netns::enter(&mut command)?;
    super::linux::sandbox::confine(&mut command, confinement)?;
    // Last: no Unix or vsock socket, which the namespace does not close.
    super::linux::seccomp::confine(command.as_std_mut())?;
    Ok((command, receiver))
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
