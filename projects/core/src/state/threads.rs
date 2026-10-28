//! Threads: one per chat. Everything else in a thread is its messages and turns.

use anyhow::{Context, Result, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use super::{State, timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: String,
    /// Who started the thread, such as `cli`.
    pub source: String,
    /// Unix timestamp in milliseconds.
    pub created_at: i64,
}

impl State {
    pub async fn create_thread(&self, id: &str, source: &str) -> Result<Thread> {
        ensure!(!id.trim().is_empty(), "thread ID is empty");
        ensure!(!source.trim().is_empty(), "thread source is empty");
        let timestamp = timestamp_ms()?;
        let row = sqlx::query(
            "INSERT INTO threads (id, source, created_at) VALUES (?, ?, ?) RETURNING *",
        )
        .bind(id)
        .bind(source)
        .bind(timestamp)
        .fetch_one(&self.pool)
        .await
        .context("create thread")?;
        let thread = decode_thread(row)?;
        Ok(thread)
    }
}

fn decode_thread(row: SqliteRow) -> Result<Thread> {
    Ok(Thread {
        id: row.try_get("id")?,
        source: row.try_get("source")?,
        created_at: row.try_get("created_at")?,
    })
}
