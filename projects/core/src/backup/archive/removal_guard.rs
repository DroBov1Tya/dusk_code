//! The lock around `duskcode remove`, held through all deletions.

use std::{
    fs::{self},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use super::{
    PENDING_RESTORE, RESTORE_LOCK,
    files::{ensure_same_file, sync_directory},
    restore_guard::{
        RestoreGuard, require_no_pending_restore, restore_control_root, restore_guard,
    },
    verify::inspect_tree_path,
};

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
