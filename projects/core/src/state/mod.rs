//! SQLite session records and in-memory events emitted after successful writes.

mod event_logs;
mod jobs;
mod messages;
mod threads;
mod turns;

pub use jobs::{Job, JobInput, JobOutcome, JobStatus};
pub use messages::{Message, MessageAttachment, MessageInput, MessageRole};
pub use threads::{ApprovalMode, ThreadInput, ThreadMetadata};
pub use turns::{Turn, TurnStatus};

use std::{
    io::ErrorKind,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, ensure};
use sqlx::{ConnectOptions, SqlitePool, migrate::Migrator, sqlite::SqliteConnectOptions};
use tokio::sync::{Mutex, broadcast};

use crate::events::{EventReceiver, EventSender};

const EVENT_CAPACITY: usize = 256;
static MIGRATOR: Migrator = sqlx::migrate!("src/state/migrations");

pub struct State {
    pool: SqlitePool,
    events: EventSender,
    // Preserve write/event ordering for callers sharing this state instance.
    writes: Mutex<()>,
}

fn timestamp_ms() -> Result<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock precedes Unix epoch")?;
    i64::try_from(elapsed.as_millis()).context("timestamp exceeds SQLite integer range")
}

impl State {
    /// Opens a file-backed database, creating parent directories and applying migrations.
    /// The caller owns the path; no database is placed in the source tree by default.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_password(path.as_ref(), None).await
    }

    /// Opens an existing encrypted database. The password remains in pool memory for reconnects.
    pub async fn open_encrypted(path: impl AsRef<Path>, password: &str) -> Result<Self> {
        ensure!(!password.is_empty(), "database password is empty");
        ensure!(!password.contains('\0'), "database password contains NUL");
        ensure!(
            tokio::fs::metadata(path.as_ref()).await?.is_file(),
            "database does not exist"
        );
        Self::open_with_password(path.as_ref(), Some(password)).await
    }

    pub(crate) async fn open_with_password(path: &Path, password: Option<&str>) -> Result<Self> {
        ensure!(!path.as_os_str().is_empty(), "state database path is empty");

        // Check before connecting so opening an existing file never creates a replacement.
        let exists = match tokio::fs::metadata(path).await {
            Ok(metadata) => {
                ensure!(metadata.is_file(), "state database path is not a file");
                true
            }
            Err(error) if error.kind() == ErrorKind::NotFound => false,
            Err(error) => return Err(error).context("inspect state database path"),
        };

        if !exists {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                tokio::fs::create_dir_all(parent)
                    .await
                    .context("create state database directory")?;
            }
        }

        let mut options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(!exists)
            .foreign_keys(true)
            .disable_statement_logging();
        if let Some(password) = password {
            // SQLCipher requires key before all other pragmas; SQLx reserves its first slot.
            options = options.pragma("key", format!("'{}'", password.replace('\'', "''")));
        }
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .context("open state database")?;
        if password.is_some() {
            let version: String = sqlx::query_scalar("PRAGMA cipher_version")
                .fetch_one(&pool)
                .await
                .context("SQLCipher unavailable")?;
            ensure!(!version.is_empty(), "SQLCipher unavailable");
            sqlx::query("SELECT count(*) FROM sqlite_master")
                .execute(&pool)
                .await
                .context("unlock state database")?;
        }
        MIGRATOR
            .run(&pool)
            .await
            .context("migrate state database")?;

        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Ok(Self {
            pool,
            events,
            writes: Mutex::new(()),
        })
    }

    /// Events are live only: durable session records can be loaded separately, without replay.
    pub fn subscribe(&self) -> EventReceiver {
        self.events.subscribe()
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}
