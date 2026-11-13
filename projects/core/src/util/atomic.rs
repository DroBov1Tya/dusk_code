//! Replacing a file whole: the new bytes go to a file of their own beside it, which is
//! then moved over it, so a crash leaves the old file or the new one, never half of one.

use std::{
    fs::{self, OpenOptions, Permissions},
    io::Write,
    path::Path,
};

use anyhow::{Context, Result};

/// Writes `bytes` to `path`, readable by the owner only. Each call stages under a name
/// of its own, so two writers never share a staging file.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    replace(path, bytes, None)
}

/// Writes `bytes` to `path` with the permissions `had`, those of the file it replaces;
/// a new file gets what the user's umask gives any new file.
pub fn write_keeping(path: &Path, bytes: &[u8], had: Option<Permissions>) -> Result<()> {
    replace(path, bytes, Some(had))
}

/// `keep` is none for a private file, else the permissions to carry over, if any.
fn replace(path: &Path, bytes: &[u8], keep: Option<Option<Permissions>>) -> Result<()> {
    let name = path
        .file_name()
        .context("cannot write a file without a name")?
        .to_string_lossy();
    let staging = path.with_file_name(format!(".{name}.{}.saving", uuid::Uuid::now_v7()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Private while it is staged, unless it is a new file of the user's own.
        options.mode(if keep == Some(None) { 0o666 } else { 0o600 });
    }
    let written = options
        .open(&staging)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            if let Some(Some(had)) = keep {
                file.set_permissions(had)?;
            }
            file.sync_all()
        })
        .and_then(|()| fs::rename(&staging, path));
    if written.is_err() {
        let _ = fs::remove_file(&staging);
    }
    written.with_context(|| format!("cannot write {}", path.display()))
}
