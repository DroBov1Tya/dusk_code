//! The core lock: an exclusive `flock` on `core.lock` held by the owner for the life of
//! the core. The kernel drops it with the process, so a crash leaves no stale owner. The
//! line written in it is for people and messages only, never trusted for a decision.

use std::{
    fs::{File, OpenOptions, Permissions, TryLockError},
    io::Read,
    os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use dusk_protocol::client::OwnerKind;
use nix::libc::{O_NOFOLLOW, O_NONBLOCK};
use serde::{Deserialize, Serialize};

use super::files::{RunFiles, euid, owned_by_us};

/// The most of `core.lock` that is read; its one line is far shorter.
const MAX_INFO: u64 = 4096;

/// Who holds the lock, as the holder wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    pub version: String,
    pub protocol: u32,
    pub owner: OwnerKind,
    pub started_at: i64,
    pub exe: PathBuf,
}

/// The lock while this process holds it; dropping it lets the next core start.
pub struct CoreLock {
    file: File,
    path: PathBuf,
}

impl CoreLock {
    /// Takes the lock, or `None` while another process holds it.
    pub fn try_take(files: &RunFiles) -> Result<Option<Self>> {
        let file = open(files, true)?;
        let meta = file.metadata().context("cannot inspect the core lock")?;
        ensure!(
            meta.is_file() && owned_by_us(&meta, euid()),
            "the core lock must be a regular file of this user"
        );
        if meta.mode() & 0o077 != 0 {
            file.set_permissions(Permissions::from_mode(0o600))
                .context("cannot narrow the core lock")?;
        }
        match file.try_lock() {
            Ok(()) => Ok(Some(Self {
                file,
                path: files.lock.clone(),
            })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error).context("cannot take the core lock"),
        }
    }

    /// The lock file held.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Replaces the line in the lock with `info`.
    pub fn write_info(&self, info: &LockInfo) -> Result<()> {
        let mut line = serde_json::to_string(info)?;
        line.push('\n');
        self.file
            .set_len(0)
            .and_then(|()| self.file.write_all_at(line.as_bytes(), 0))
            .and_then(|()| self.file.sync_data())
            .context("cannot write the core lock")
    }
}

/// What the holder of the lock wrote, when there is a readable line.
pub fn read_info(files: &RunFiles) -> Option<LockInfo> {
    let file = open(files, false).ok()?;
    let mut text = String::new();
    file.take(MAX_INFO).read_to_string(&mut text).ok()?;
    serde_json::from_str(text.lines().next()?).ok()
}

fn open(files: &RunFiles, create: bool) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .write(create)
        .create(create)
        .truncate(false)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK)
        .open(&files.lock)
        .context("cannot open the core lock")
}
