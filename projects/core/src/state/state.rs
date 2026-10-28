//! The state database handle.

use std::{
    io::ErrorKind,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, ensure};
use sqlx::{ConnectOptions, SqlitePool, migrate::Migrator, sqlite::SqliteConnectOptions};

static MIGRATOR: Migrator = sqlx::migrate!("src/state/migrations");

// One connection, so writes never interleave: each one runs in its own transaction.
pub struct State {
    pub(super) pool: SqlitePool,
}

impl State {
    /// Opens or creates the database. The caller owns the path.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        ensure!(!path.as_os_str().is_empty(), "state database path is empty");
        // Checked before connecting so opening an existing file never creates a replacement.
        let exists = match tokio::fs::metadata(path).await {
            Ok(metadata) => {
                ensure!(metadata.is_file(), "state database path is not a file");
                true
            }
            Err(error) if error.kind() == ErrorKind::NotFound => false,
            Err(error) => return Err(error).context("inspect state database path"),
        };
        if !exists {
            create_parent_dir(path).await?;
        }

        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(!exists)
            .foreign_keys(true)
            .disable_statement_logging();
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .context("open state database")?;
        MIGRATOR
            .run(&pool)
            .await
            .context("migrate state database")?;
        Ok(Self { pool })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

pub(super) fn timestamp_ms() -> Result<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock precedes Unix epoch")?;
    i64::try_from(elapsed.as_millis()).context("timestamp exceeds SQLite integer range")
}

async fn create_parent_dir(path: &Path) -> Result<()> {
    match path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        Some(parent) => tokio::fs::create_dir_all(parent)
            .await
            .context("create state database directory"),
        None => Ok(()),
    }
}
