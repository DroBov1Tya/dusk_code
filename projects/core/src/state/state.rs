//! The state database handle: connection pool, write ordering and live events.

use std::{
    fmt,
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

/// Writes through one handle publish their events in write order.
pub struct State {
    pub(super) pool: SqlitePool,
    pub(super) events: EventSender,
    pub(super) writes: Mutex<()>,
}

/// SQLCipher key. Never serialized, logged or included in events.
pub struct DatabasePassword(String);

impl State {
    /// Opens or creates a plain database. The caller owns the path.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::connect(path.as_ref(), None).await
    }

    /// Creates a new encrypted database; an existing file is never overwritten or converted.
    pub async fn create_encrypted(
        path: impl AsRef<Path>,
        password: &DatabasePassword,
    ) -> Result<Self> {
        let path = path.as_ref();
        create_parent_dir(path).await?;
        tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .await
            .context("reserve new state database")?;
        Self::connect(path, Some(password)).await
    }

    /// The password stays in pool memory for reconnects.
    pub async fn open_encrypted(
        path: impl AsRef<Path>,
        password: &DatabasePassword,
    ) -> Result<Self> {
        let path = path.as_ref();
        ensure!(
            tokio::fs::metadata(path).await?.is_file(),
            "database does not exist"
        );
        Self::connect(path, Some(password)).await
    }

    /// Events are live only; durable records are loaded separately, without replay.
    pub fn subscribe(&self) -> EventReceiver {
        self.events.subscribe()
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn connect(path: &Path, password: Option<&DatabasePassword>) -> Result<Self> {
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

        let mut options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(!exists)
            .foreign_keys(true)
            .disable_statement_logging();
        if let Some(password) = password {
            // SQLCipher requires the key before all other pragmas; SQLx reserves its first slot.
            options = options.pragma("key", format!("'{}'", password.0.replace('\'', "''")));
        }
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .context("open state database")?;
        if password.is_some() {
            unlock(&pool).await?;
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
}

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

/// A wrong key only fails on the first read, so read once to report it here.
async fn unlock(pool: &SqlitePool) -> Result<()> {
    let version: String = sqlx::query_scalar("PRAGMA cipher_version")
        .fetch_one(pool)
        .await
        .context("SQLCipher unavailable")?;
    ensure!(!version.is_empty(), "SQLCipher unavailable");
    sqlx::query("SELECT count(*) FROM sqlite_master")
        .execute(pool)
        .await
        .context("unlock state database")?;
    Ok(())
}
