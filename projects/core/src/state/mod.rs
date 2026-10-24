//! SQLite metadata and in-memory events emitted after successful writes.

mod event_logs;
mod threads;

pub use threads::{ApprovalMode, ThreadInput, ThreadMetadata};

use std::{io::ErrorKind, path::Path};

use anyhow::{Context, Result, ensure};
use sqlx::{SqlitePool, migrate::Migrator, sqlite::SqliteConnectOptions};
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

impl State {
    /// Opens a file-backed database, creating parent directories and applying migrations.
    /// The caller owns the path; no database is placed in the source tree by default.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
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

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(!exists)
            .foreign_keys(true);
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .context("open state database")?;
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

    /// Events are live only: request receipts can be loaded separately, without replay.
    pub fn subscribe(&self) -> EventReceiver {
        self.events.subscribe()
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}
