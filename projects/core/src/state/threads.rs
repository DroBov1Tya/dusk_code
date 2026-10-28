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
    /// `None` until the first message.
    pub title: Option<String>,
    /// When the newest message was stored, or `created_at` before the first one.
    pub active_at: i64,
}

impl State {
    pub async fn create_thread(&self, id: &str, source: &str) -> Result<Thread> {
        ensure!(!id.trim().is_empty(), "thread ID is empty");
        ensure!(!source.trim().is_empty(), "thread source is empty");
        let timestamp = timestamp_ms()?;
        sqlx::query("INSERT INTO threads (id, source, created_at) VALUES (?, ?, ?)")
            .bind(id)
            .bind(source)
            .bind(timestamp)
            .execute(&self.pool)
            .await
            .context("create thread")?;
        self.load_thread(id).await?.context("thread was not stored")
    }

    pub async fn load_thread(&self, id: &str) -> Result<Option<Thread>> {
        sqlx::query(
            "SELECT threads.*, COALESCE(
                (SELECT MAX(created_at) FROM messages WHERE thread_id = threads.id),
                threads.created_at) AS active_at
             FROM threads WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("load thread")?
        .map(decode_thread)
        .transpose()
    }

    /// Every thread, the most recently active first.
    pub async fn list_threads(&self) -> Result<Vec<Thread>> {
        sqlx::query(
            "SELECT threads.*, COALESCE(
                (SELECT MAX(created_at) FROM messages WHERE thread_id = threads.id),
                threads.created_at) AS active_at
             FROM threads ORDER BY active_at DESC, id DESC",
        )
        .fetch_all(&self.pool)
        .await
        .context("list threads")?
        .into_iter()
        .map(decode_thread)
        .collect()
    }

    pub async fn set_title(&self, id: &str, title: &str) -> Result<()> {
        ensure!(!title.trim().is_empty(), "title is empty");
        sqlx::query("UPDATE threads SET title = ? WHERE id = ?")
            .bind(title)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("set thread title")?;
        Ok(())
    }
}

fn decode_thread(row: SqliteRow) -> Result<Thread> {
    Ok(Thread {
        id: row.try_get("id")?,
        source: row.try_get("source")?,
        created_at: row.try_get("created_at")?,
        title: row.try_get("title")?,
        active_at: row.try_get("active_at")?,
    })
}
