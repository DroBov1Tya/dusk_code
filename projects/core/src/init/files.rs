//! The private-file boundary: files are replaced whole and read without following a link.
//!
//! A write stages the new bytes in a file of their own beside the target and moves it over,
//! so a crash leaves the old file or the new one, never half of one. A read refuses a
//! symlink swapped in for the last component and never blocks on a FIFO.

use std::{
    fs::{self, File, OpenOptions, Permissions},
    io::{Read, Write},
    path::Path,
};

use anyhow::{Context, Result, ensure};

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

/// Reads a regular file. The last path component is not followed if it is a symlink and a
/// FIFO does not block, so a file swapped after the scope check cannot redirect the read.
pub fn read_regular(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let file = open_no_follow(OpenOptions::new().read(true), path)?;
    let metadata = file.metadata().context("cannot read file")?;
    ensure!(metadata.is_file(), "not a regular file");
    ensure!(
        metadata.len() <= max_bytes,
        "file is larger than {max_bytes} bytes"
    );
    let mut bytes = Vec::new();
    // The file may grow after the size check.
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .context("cannot read file")?;
    ensure!(
        bytes.len() as u64 <= max_bytes,
        "file is larger than {max_bytes} bytes"
    );
    Ok(bytes)
}

pub(crate) fn open_no_follow(options: &mut OpenOptions, path: &Path) -> Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    options.open(path).context("cannot open file")
}
