//! The record of what is attached to a chat; the files are in its workspace.

use anyhow::{Context, Result};
use sqlx::{FromRow, Row, sqlite::SqliteRow};

use super::{State, timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub source_id: String,
    pub name: String,
    pub file: String,
    pub bytes: u64,
    pub pinned: bool,
    pub delivered: bool,
}

impl FromRow<'_, SqliteRow> for Source {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            source_id: row.try_get("source_id")?,
            name: row.try_get("name")?,
            file: row.try_get("file")?,
            bytes: row.try_get::<i64, _>("bytes")?.max(0) as u64,
            pinned: row.try_get("pinned")?,
            delivered: row.try_get("delivered")?,
        })
    }
}

impl State {
    pub async fn add_source(&self, thread_id: &str, source: &Source) -> Result<()> {
        sqlx::query(
            "INSERT INTO sources (source_id, thread_id, name, file, bytes, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&source.source_id)
        .bind(thread_id)
        .bind(&source.name)
        .bind(&source.file)
        .bind(source.bytes as i64)
        .bind(timestamp_ms()?)
        .execute(&self.pool)
        .await
        .context("store the source")?;
        Ok(())
    }

    /// The chat's sources, oldest first.
    pub async fn sources(&self, thread_id: &str) -> Result<Vec<Source>> {
        sqlx::query_as("SELECT * FROM sources WHERE thread_id = ? ORDER BY created_at, source_id")
            .bind(thread_id)
            .fetch_all(&self.pool)
            .await
            .context("list the sources")
    }

    pub async fn pin_source(&self, thread_id: &str, id: &str, pinned: bool) -> Result<()> {
        sqlx::query("UPDATE sources SET pinned = ? WHERE thread_id = ? AND source_id = ?")
            .bind(pinned)
            .bind(thread_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("pin the source")?;
        Ok(())
    }

    /// Forgets a source and says which file it was.
    pub async fn remove_source(&self, thread_id: &str, id: &str) -> Result<Option<String>> {
        sqlx::query_scalar(
            "DELETE FROM sources WHERE thread_id = ? AND source_id = ? RETURNING file",
        )
        .bind(thread_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .context("remove the source")
    }

    pub async fn mark_sources_delivered(&self, ids: &[String]) -> Result<()> {
        for id in ids {
            sqlx::query("UPDATE sources SET delivered = 1 WHERE source_id = ?")
                .bind(id)
                .execute(&self.pool)
                .await
                .context("mark the source delivered")?;
        }
        Ok(())
    }
}
