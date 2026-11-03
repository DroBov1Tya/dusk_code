//! Replacing a file whole: the new bytes go to a file of their own beside it, which is
//! then moved over it, so a crash leaves the old file or the new one, never half of one.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use anyhow::{Context, Result};

/// Writes `bytes` to `path`, readable by the owner only. Each call stages under a name
/// of its own, so two writers never share a staging file.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
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
        options.mode(0o600);
    }
    let written = options
        .open(&staging)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .and_then(|()| fs::rename(&staging, path));
    if written.is_err() {
        let _ = fs::remove_file(&staging);
    }
    written.with_context(|| format!("cannot write {}", path.display()))
}
