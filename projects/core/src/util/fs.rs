//! Reading regular files without following a link swapped in for one.

use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::Path,
};

use anyhow::{Context, Result, ensure};

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

/// Control characters in names or lines would reach the model and terminals as markup.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() && c != '\t' { '?' } else { c })
        .collect()
}

pub(crate) fn open_no_follow(options: &mut OpenOptions, path: &Path) -> Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    options.open(path).context("cannot open file")
}

/// A leading `~/` as people write it; the rest stays as typed.
pub fn expand_home(path: &str) -> std::path::PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => std::path::PathBuf::from(home).join(rest),
        _ => std::path::PathBuf::from(path),
    }
}
