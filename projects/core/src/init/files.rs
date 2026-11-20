//! The private-file boundary: files are replaced whole and read without following a link,
//! and files and folders are made private to the user.
//!
//! A write stages the new bytes in a file of their own beside the target and moves it over,
//! so a crash leaves the old file or the new one, never half of one. A read refuses a
//! symlink swapped in for the last component and never blocks on a FIFO.

use std::{
    fs::{self, DirBuilder, File, OpenOptions, Permissions},
    io::{Read, Write},
    path::Path,
};

use anyhow::{Context, Result, anyhow, ensure};

/// What a replaced file is given: a mode of its own, or the permissions of the file it
/// replaces (none for a new file of the user's own).
enum Mode {
    Private(u32),
    Keeping(Option<Permissions>),
}

/// Writes `bytes` to `path`, readable by the owner only. Each call stages under a name
/// of its own, so two writers never share a staging file.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    replace(path, bytes, Mode::Private(0o600))
}

/// `write_private` for a program: readable and runnable by the owner only.
pub fn write_private_program(path: &Path, bytes: &[u8]) -> Result<()> {
    replace(path, bytes, Mode::Private(0o700))
}

/// Writes `bytes` to `path` with the permissions `had`, those of the file it replaces;
/// a new file gets what the user's umask gives any new file.
pub fn write_keeping(path: &Path, bytes: &[u8], had: Option<Permissions>) -> Result<()> {
    replace(path, bytes, Mode::Keeping(had))
}

fn replace(path: &Path, bytes: &[u8], mode: Mode) -> Result<()> {
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
        options.mode(match mode {
            Mode::Private(mode) => mode,
            Mode::Keeping(None) => 0o666,
            Mode::Keeping(Some(_)) => 0o600,
        });
    }
    let written = options
        .open(&staging)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            if let Mode::Keeping(Some(had)) = &mode {
                file.set_permissions(had.clone())?;
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
    read_bounded(
        open_no_follow(OpenOptions::new().read(true), path)?,
        max_bytes,
    )
}

/// Reads `file`, opened by the caller, when it is a regular file of at most `max_bytes`.
pub fn read_bounded(file: File, max_bytes: u64) -> Result<Vec<u8>> {
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

/// Makes `directory` and any missing parents, private to the user, and refuses one that is a
/// symlink.
pub fn create_private_dir(directory: &Path) -> Result<()> {
    private_dir_builder()
        .recursive(true)
        .create(directory)
        .context("cannot create application directory")?;
    let metadata = fs::symlink_metadata(directory)
        .map_err(|_| anyhow!("cannot inspect application directory"))?;
    ensure!(
        metadata.is_dir(),
        "application directory must not be a symlink"
    );
    Ok(())
}

/// Makes `directory`, private to the user, in a parent that exists; anything already at
/// its path, a symlink too, is an error.
pub fn new_private_dir(directory: &Path) -> Result<()> {
    private_dir_builder()
        .create(directory)
        .with_context(|| format!("create private directory {}", directory.display()))
}

fn private_dir_builder() -> DirBuilder {
    #[allow(unused_mut)]
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
}

/// A new file at `path`, readable by the owner only; anything already there, a symlink
/// too, is an error, whose `std::io::Error` says so.
pub fn new_private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("create private file {}", path.display()))
}

/// Removes the folders above `file` while they are empty, from the nearest up, and stops at
/// the first one `keep` names or that still holds anything.
pub fn remove_empty_parents(file: &Path, keep: impl Fn(&Path) -> bool) {
    let mut folder = file.parent();
    while let Some(parent) = folder {
        if keep(parent) || fs::remove_dir(parent).is_err() {
            break;
        }
        folder = parent.parent();
    }
}
