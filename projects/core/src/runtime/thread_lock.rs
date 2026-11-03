//! One process per thread. Two processes running one thread would answer the same
//! messages twice, and each would take the other's running turn for one a crash left.
//! A lock on a file in the thread's workspace says which process has the thread; the
//! system lets it go when that process ends, even when it is killed.

use std::{
    fs::{File, OpenOptions, TryLockError},
    path::Path,
};

use anyhow::{Context, Result, bail};

const LOCK_FILE: &str = ".dusk.lock";

/// Held for as long as a runner runs the thread.
pub struct ThreadLock {
    _file: File,
}

impl ThreadLock {
    pub fn take(workspace: &Path) -> Result<Self> {
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(workspace.join(LOCK_FILE))
            .context("open the thread lock")?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => bail!("the thread is open in another dusk"),
            Err(TryLockError::Error(error)) => Err(error).context("lock the thread"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    #[test]
    fn a_thread_has_one_holder_until_it_lets_go() {
        let workspace = temp_dir("thread_lock");
        std::fs::create_dir_all(&workspace).unwrap();
        let first = ThreadLock::take(&workspace).unwrap();
        assert!(ThreadLock::take(&workspace).is_err());
        drop(first);
        assert!(ThreadLock::take(&workspace).is_ok());
    }
}
