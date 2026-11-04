//! The locks around a restore and around `duskcode remove`, and the paths they protect.

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

use crate::init::paths::Paths;

use super::{
    PENDING_RESTORE, RESTORE_LOCK,
    files::{ensure_same_file, sync_directory},
    verify::{directory_tree, inspect_tree_path},
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

pub fn absolute_for_overlap(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    Ok(std::env::current_dir()
        .context(
            "cannot resolve a relative data path because the current directory is unavailable",
        )?
        .join(path))
}

pub fn paths_overlap(left: &Path, right: &Path) -> bool {
    if left.starts_with(right) || right.starts_with(left) {
        return true;
    }
    match (physical_path(left), physical_path(right)) {
        (Some(left), Some(right)) => left.starts_with(&right) || right.starts_with(&left),
        _ => false,
    }
}

/// Resolve existing ancestors too: a symlink alias must still coordinate when the
/// final data root has not been created yet. A disjoint inaccessible HOME is harmless.
pub fn physical_path(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if let Ok(mut physical) = fs::canonicalize(ancestor) {
            // Preserve symlink semantics in the existing prefix, then normalize the
            // missing suffix. A missing `sub/../state.db` still belongs to its parent.
            for component in path.strip_prefix(ancestor).ok()?.components() {
                match component {
                    Component::Normal(name) => physical.push(name),
                    Component::CurDir => {}
                    Component::ParentDir => {
                        physical.pop();
                    }
                    Component::Prefix(_) | Component::RootDir => return None,
                }
            }
            return Some(physical);
        }
    }
    None
}

/// Removal uses the same exclusive lock as restore. The command must retain this
/// guard through all deletions and must leave the coordination inode linked in place.
pub struct RemovalGuard {
    pub guard: RestoreGuard,
    pub lock_path: PathBuf,
}

impl RemovalGuard {
    pub fn acquire() -> Result<RemovalGuard> {
        let guard = restore_guard(true)?;
        require_no_pending_restore()?;
        Ok(RemovalGuard {
            guard,
            lock_path: restore_control_root()?.join(RESTORE_LOCK),
        })
    }

    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    pub fn validate_application_data_root(&self, directory: &Path) -> Result<()> {
        self.removal_directory(directory).map(|_| ())
    }

    /// A default install shares the restore control directory. Delete its other
    /// children, never that directory or the live lock, so another process cannot
    /// open a replacement inode while this exclusive owner is still deleting data.
    pub fn remove_application_data(&self, directory: &Path) -> Result<()> {
        require_no_pending_restore()?;
        let Some((directory, control)) = self.removal_directory(directory)? else {
            return Ok(());
        };
        if directory == control {
            for entry in fs::read_dir(&directory)? {
                let entry = entry?;
                if entry.file_name() == RESTORE_LOCK {
                    continue;
                }
                ensure!(
                    entry.file_name() != PENDING_RESTORE,
                    "pending restore journal appeared; removal stopped without deleting it"
                );
                let metadata = fs::symlink_metadata(entry.path())?;
                if metadata.is_dir() {
                    fs::remove_dir_all(entry.path())?;
                } else {
                    fs::remove_file(entry.path())?;
                }
            }
            sync_directory(&control)?;
        } else {
            fs::remove_dir_all(&directory)
                .with_context(|| format!("cannot remove {}", directory.display()))?;
        }
        Ok(())
    }

    fn removal_directory(&self, directory: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
        inspect_tree_path(directory)?;
        match fs::symlink_metadata(directory) {
            Ok(metadata) => ensure!(
                metadata.is_dir(),
                "application data root must be a real directory"
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("inspect application data before removal"),
        }
        // Confirm the pathname still names the held inode before allowing deletion.
        ensure_same_file(
            &fs::symlink_metadata(&self.lock_path)?,
            &self.guard._file.metadata()?,
        )?;
        let control = fs::canonicalize(
            self.lock_path
                .parent()
                .context("restore lock has no parent")?,
        )?;
        let directory = fs::canonicalize(directory)?;
        ensure!(
            directory == control || !control.starts_with(&directory),
            "refusing to recursively remove an application data root containing the restore control directory"
        );
        Ok(Some((directory, control)))
    }
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
