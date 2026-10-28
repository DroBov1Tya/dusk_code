//! Bootstrap transport, independent of thread requests and durable execution receipts.

use std::{fmt, path::PathBuf};

use anyhow::{Context, Result, ensure};

use crate::{events::DatabaseCreated, state::State};

/// Caller-supplied password. Never serialized or included in events.
pub struct DatabasePassword(String);

impl DatabasePassword {
    pub fn new(password: String) -> Result<Self> {
        ensure!(!password.is_empty(), "database password is empty");
        ensure!(!password.contains('\0'), "database password contains NUL");
        Ok(Self(password))
    }
}

impl fmt::Debug for DatabasePassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DatabasePassword([REDACTED])")
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum DatabaseTask {
    CreateEncryptedDatabase {
        path: PathBuf,
        password: DatabasePassword,
    },
}

pub struct DatabaseTransport;

impl DatabaseTransport {
    /// Creates a new file exclusively; existing databases are never overwritten or converted.
    /// The returned event is a bootstrap result, not an event_logs receipt.
    pub async fn handle(task: DatabaseTask) -> Result<(State, DatabaseCreated)> {
        let DatabaseTask::CreateEncryptedDatabase { path, password } = task;
        ensure!(!path.as_os_str().is_empty(), "state database path is empty");
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(parent)
                .await
                .context("create state database directory")?;
        }
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
            .context("reserve new state database")?;
        drop(file);
        let state = State::open_with_password(&path, Some(&password.0)).await?;
        let event = DatabaseCreated;
        Ok((state, event))
    }
}
