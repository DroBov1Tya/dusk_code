//! Where temporary chats keep their files: a private folder in the system's temporary
//! folder, removed when this process ends. A lock held for as long as the process lives
//! tells the next start which folders a crash left behind.

use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::init::create_private_dir;

const PREFIX: &str = "duskcode-temp-";
const ALIVE: &str = "alive.lock";
/// A folder with no lock file yet may be one another process is still making.
const BEING_MADE: Duration = Duration::from_secs(60);

pub struct TemporaryRoot {
    pub path: PathBuf,
    _alive: File,
}

impl TemporaryRoot {
    /// Removes the folders dead processes left, then makes this one's.
    pub fn make() -> Result<Self> {
        let base = std::env::temp_dir();
        sweep(&base);
        let path = base.join(format!("{PREFIX}{}", Uuid::now_v7()));
        create_private_dir(&path)?;
        let alive = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path.join(ALIVE))
            .context("mark the temporary folder as in use")?;
        alive
            .try_lock()
            .context("lock the temporary folder's mark")?;
        Ok(Self {
            path,
            _alive: alive,
        })
    }

    /// The file whose lock says this process lives; it exists for as long as the folder.
    pub fn mark(&self) -> PathBuf {
        self.path.join(ALIVE)
    }
}

impl Drop for TemporaryRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A folder is abandoned when nobody holds its lock. The temporary folder may be shared
/// with other users, so only real folders are touched, never a link to one.
fn sweep(base: &Path) {
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(PREFIX) {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        let abandoned = match File::open(path.join(ALIVE)) {
            Ok(alive) => alive.try_lock().is_ok(),
            Err(_) => metadata
                .modified()
                .ok()
                .and_then(|made| made.elapsed().ok())
                .is_some_and(|age| age > BEING_MADE),
        };
        if abandoned {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_goes_with_its_owner_and_a_live_one_is_left_alone() {
        let first = TemporaryRoot::make().unwrap();
        let kept = first.path.clone();
        fs::write(kept.join("note"), "secret").unwrap();
        // Another start sweeps: the first is alive, so it stays.
        let second = TemporaryRoot::make().unwrap();
        assert!(kept.join("note").exists());
        drop(first);
        assert!(!kept.exists());
        drop(second);
    }
}
