//! Small file helpers: bounded reads, private creation, syncing and the staging directory.

use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::BUFFER_BYTES;
use crate::init::files::{new_private_dir, open_no_follow};

pub fn sync_directory(directory: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(directory)?
            .sync_all()
            .with_context(|| format!("sync application directory {}", directory.display()))?;
    }
    // std does not expose a portable directory flush on other platforms. File syncs
    // and atomic rename still apply; full power-loss durability depends on the OS/FS.
    #[cfg(not(unix))]
    {
        let _ = directory;
    }
    Ok(())
}

/// A restore journal file of at most `maximum` bytes, read as `open_regular_input` opens it.
pub fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    crate::init::files::read_bounded(open_regular_input(path)?, maximum)
        .context("read a restore journal file")
}

pub fn open_regular_input(path: &Path) -> Result<File> {
    let before =
        fs::symlink_metadata(path).context("inspect archive or journal input before opening")?;
    ensure!(
        before.is_file(),
        "archive or journal input must be a regular file, not a symlink or special file"
    );
    // A FIFO substituted after the metadata check cannot block open, and a final symlink
    // substituted in that window cannot redirect it to a different target.
    let file = open_no_follow(OpenOptions::new().read(true), path)
        .context("open regular archive or journal input")?;
    ensure_same_file(&before, &file.metadata()?)?;
    Ok(file)
}

pub fn executable(metadata: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

pub fn set_executable(file: &File, executable: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(if executable {
            0o700
        } else {
            0o600
        }))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (file, executable);
    }
    Ok(())
}

pub fn ensure_same_file(before: &Metadata, opened: &Metadata) -> Result<()> {
    ensure!(opened.is_file(), "archive source changed to a special file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            before.dev() == opened.dev() && before.ino() == opened.ino(),
            "archive source changed while opening it"
        );
    }
    ensure!(
        before.len() == opened.len(),
        "archive source changed while opening it"
    );
    Ok(())
}

pub fn copy_hash(input: &mut impl Read, output: &mut impl Write) -> Result<(u64, String)> {
    let mut buffer = [0; BUFFER_BYTES];
    let mut hash = Sha256::new();
    let mut copied = 0u64;
    loop {
        let bytes = input.read(&mut buffer)?;
        if bytes == 0 {
            break;
        }
        output.write_all(&buffer[..bytes])?;
        hash.update(&buffer[..bytes]);
        copied = copied
            .checked_add(bytes as u64)
            .context("file size overflow")?;
    }
    let hash = dusk_protocol::text::hex(&hash.finalize());
    Ok((copied, hash))
}

pub struct Temporary {
    pub path: PathBuf,
    pub keep: bool,
    pub offline: bool,
}

impl Temporary {
    pub fn new(parent: &Path, purpose: &str) -> Result<Self> {
        let path = parent.join(format!(".duskcode-{purpose}-{}", Uuid::now_v7()));
        new_private_dir(&path)?;
        Ok(Self {
            path,
            keep: false,
            offline: purpose.starts_with("restore"),
        })
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        let Err(error) = fs::remove_dir_all(&self.path) else {
            return;
        };
        if error.kind() == ErrorKind::NotFound {
            return;
        }
        tracing::warn!(
            "cannot remove private archive staging directory {}: {error}",
            self.path.display()
        );
        if self.offline {
            eprintln!(
                "duskcode: private restore staging remains at {}: {error}",
                self.path.display()
            );
        }
    }
}
