//! A kernel-held lease identifies one runtime, including while its thread is closed.
//! Lease files are never reused or removed while jobs could still need crash recovery.

use std::{
    collections::HashSet,
    fs::{File, OpenOptions, TryLockError},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use uuid::Uuid;

pub(super) struct JobLease {
    pub id: String,
    pub path: PathBuf,
    _file: File,
}

pub(super) enum Owner {
    Live,
    Abandoned(JobLease),
    Unknown,
}

impl JobLease {
    pub fn take(database: &Path) -> Result<Self> {
        let database = database
            .canonicalize()
            .context("locate job ownership directory")?;
        let directory = database
            .parent()
            .context("database has no parent directory")?
            .join(".dusk-job-leases");
        crate::init::create_private_dir(&directory)?;
        let id = Uuid::now_v7().to_string();
        let path = directory.join(format!("{id}.lock"));
        let mut options = OpenOptions::new();
        options.create_new(true).read(true).write(true);
        private(&mut options);
        let mut file = options.open(&path).context("create job owner lease")?;
        file.try_lock().context("lock job owner lease")?;
        file.write_all(id.as_bytes())
            .context("identify job owner lease")?;
        Ok(Self {
            id,
            path,
            _file: file,
        })
    }

    /// A missing, malformed or unreadable proof says nothing about owner liveness.
    /// Only acquiring the exact registered owner's lock permits interruption.
    pub fn inspect(id: &str, path: &str) -> Owner {
        if Uuid::parse_str(id).is_err() || !Path::new(path).is_absolute() {
            return Owner::Unknown;
        }
        let path = PathBuf::from(path);
        let name = format!("{id}.lock");
        if path.file_name().and_then(|name| name.to_str()) != Some(name.as_str()) {
            return Owner::Unknown;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        private(&mut options);
        let Ok(mut file) = options.open(&path) else {
            return Owner::Unknown;
        };
        if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
            return Owner::Unknown;
        }
        let mut stored = String::new();
        if (&mut file).take(37).read_to_string(&mut stored).is_err() || stored != id {
            return Owner::Unknown;
        }
        match file.try_lock() {
            Err(TryLockError::WouldBlock) => Owner::Live,
            Err(TryLockError::Error(_)) => Owner::Unknown,
            Ok(()) => Owner::Abandoned(Self {
                id: id.to_owned(),
                path,
                _file: file,
            }),
        }
    }

    /// Removes the lease files that nobody holds and no unfinished job names: a runtime
    /// leaves one per start, and they would pile up. A held lease cannot be taken, and
    /// one still being written does not read as its own ID, so a live runtime's stays.
    pub fn sweep(&self, needed: &HashSet<String>) {
        let Some(directory) = self.path.parent() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for path in entries.flatten().map(|entry| entry.path()) {
            let Some(id) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".lock"))
            else {
                continue;
            };
            if id == self.id || needed.contains(id) {
                continue;
            }
            let Some(stored) = path.to_str() else {
                continue;
            };
            if let Owner::Abandoned(abandoned) = Self::inspect(id, stored) {
                let _ = std::fs::remove_file(&abandoned.path);
            }
        }
    }

    pub fn stored_path(&self) -> Result<&str> {
        self.path
            .to_str()
            .context("job owner lease path is not UTF-8")
    }
}

fn private(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
}
