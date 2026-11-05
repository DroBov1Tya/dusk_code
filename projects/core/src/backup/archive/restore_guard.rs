//! The lock around an offline restore, and where the pending-restore record lives.

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

use crate::init::paths::Paths;

use super::{
    PENDING_RESTORE, RESTORE_LOCK,
    overlap::{absolute_for_overlap, paths_overlap},
    verify::directory_tree,
};

/// A live runtime holds a shared OS lock; offline restore and recovery require the
/// exclusive lock. The lock file is never unlinked, so SIGKILL only releases ownership.
pub struct RestoreGuard {
    pub _file: File,
}

impl RestoreGuard {
    /// The HOME-scoped transaction only owns the standard data roots. Sandboxed runtimes
    /// with disjoint roots must not create default state or depend on its pending journal.
    pub fn startup(paths: &Paths, state_db: Option<&Path>) -> Result<Option<RestoreGuard>> {
        // Resolve before the HOME decision as well: an unresolved relative DB must never
        // silently bypass coordination merely because current_dir could not be obtained.
        let state_db = state_db.map(resolve_state_db).transpose()?;
        let config_root = absolute_for_overlap(paths.config_root())?;
        let workspace_root = absolute_for_overlap(paths.workspace_root())?;
        let standard = match Paths::from_home() {
            Ok(standard) => standard,
            Err(_) => return Ok(None),
        };
        let shared = [
            Some(config_root.as_path()),
            Some(workspace_root.as_path()),
            state_db.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|actual| {
            [standard.config_root(), standard.workspace_root()]
                .iter()
                .any(|default| paths_overlap(actual, default))
        });
        if !shared {
            return Ok(None);
        }
        let guard = restore_guard(false)?;
        require_no_pending_restore()?;
        Ok(Some(guard))
    }
}

/// Pin database selection before startup can await or initialize other components.
/// The caller passes this same absolute path to both coordination and State::open.
pub fn resolve_state_db(path: &Path) -> Result<PathBuf> {
    ensure!(!path.as_os_str().is_empty(), "state database path is empty");
    absolute_for_overlap(path)
}

pub fn restore_guard(exclusive: bool) -> Result<RestoreGuard> {
    let root = restore_control_root()?;
    directory_tree(&root)?;
    let path = root.join(RESTORE_LOCK);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => ensure!(
            metadata.is_file(),
            "restore lock must be a regular file: {}",
            path.display()
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspect restore lock"),
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    let file = options.open(&path).context("open restore lock")?;
    ensure!(
        file.metadata()?.is_file(),
        "restore lock is not a regular file"
    );
    let locked = if exclusive {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    match locked {
        Ok(()) => Ok(RestoreGuard { _file: file }),
        Err(TryLockError::WouldBlock) => {
            bail!("another duskcode process is running or restoring data; close it before retrying")
        }
        Err(TryLockError::Error(error)) => {
            Err(error).context("lock duskcode data against offline restore")
        }
    }
}

// Unlike release resources, restore coordination must not move when XDG_DATA_HOME or
// DUSKCODE_RESOURCE_DIR changes while the same HOME/config/workspace roots stay in use.

pub fn restore_control_root() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    let root = PathBuf::from(home).join(".local/share/duskcode");
    ensure!(
        root.is_absolute(),
        "restore control directory must be absolute"
    );
    Ok(root)
}

pub fn pending_restore_path() -> Result<PathBuf> {
    Ok(restore_control_root()?.join(PENDING_RESTORE))
}

pub fn require_no_pending_restore() -> Result<()> {
    let pending = pending_restore_path()?;
    match fs::symlink_metadata(&pending) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("inspect restore transaction journal"),
        Ok(_) => bail!(
            "an interrupted restore blocks startup and new restores. Run duskcode restore --recover with all other duskcode processes closed. Preserve the private transaction journal at {} and its staging directories; do not delete the journal to bypass recovery",
            pending.display()
        ),
    }
}
