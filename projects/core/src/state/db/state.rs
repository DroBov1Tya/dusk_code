//! The state database handle.

use std::{
    io::ErrorKind,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, ensure};
use sqlx::{ConnectOptions, SqlitePool, migrate::Migrator, sqlite::SqliteConnectOptions};

use super::legacy;

static MIGRATOR: Migrator = sqlx::migrate!("src/state/db/migrations");

// One connection, so writes never interleave: each one runs in its own transaction.
pub struct State {
    pub pool: SqlitePool,
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
        check(&pool, path).await?;
        legacy::convert(&pool, &MIGRATOR).await?;
        legacy::convert_tasks(&pool, &MIGRATOR).await?;
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

/// Says plainly what is wrong with a file that cannot serve as the database, before any
/// step deeper down fails on it with a less helpful message.
async fn check(pool: &SqlitePool, path: &Path) -> Result<()> {
    let shown = path.display();
    sqlx::query("SELECT COUNT(*) FROM sqlite_master")
        .execute(pool)
        .await
        .with_context(|| format!("{shown} is damaged or not a dusk database"))?;
    // A write that is rolled back: SQLite opens a read-only file without a word.
    let mut transaction = pool.begin().await.context("begin the write check")?;
    sqlx::query("CREATE TABLE dusk_write_check (id INTEGER)")
        .execute(&mut *transaction)
        .await
        .with_context(|| format!("{shown} cannot be written"))?;
    transaction
        .rollback()
        .await
        .context("end the write check")?;
    Ok(())
}

pub fn timestamp_ms() -> Result<i64> {
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
