//! The record of what is attached to a chat; the files are in its workspace.

use anyhow::{Context, Result};
use sqlx::{Row, sqlite::SqliteRow};

use super::{State, timestamp_ms};

pub use dusk_protocol::thread::Source;

/// A row of `sources`; the type lives in the protocol, so it cannot be `FromRow` here.
fn source_from_row(row: SqliteRow) -> sqlx::Result<Source> {
    Ok(Source {
        source_id: row.try_get("source_id")?,
        name: row.try_get("name")?,
        file: row.try_get("file")?,
        bytes: row.try_get::<i64, _>("bytes")?.max(0) as u64,
        pinned: row.try_get("pinned")?,
        delivered: row.try_get("delivered")?,
        plan: row.try_get::<Option<i64>, _>("plan_written_at")?.is_some(),
    })
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
        sqlx::query("SELECT * FROM sources WHERE thread_id = ? ORDER BY created_at, source_id")
            .bind(thread_id)
            .try_map(source_from_row)
            .fetch_all(&self.pool)
            .await
            .context("list the sources")
    }

    /// The new size of a source's file, after a tool rewrote it.
    pub async fn set_source_bytes(&self, thread_id: &str, id: &str, bytes: u64) -> Result<()> {
        sqlx::query("UPDATE sources SET bytes = ? WHERE thread_id = ? AND source_id = ?")
            .bind(i64::try_from(bytes).context("source size is negative")?)
            .bind(thread_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("update the source size")?;
        Ok(())
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

    /// Marks the source a plan written just now; the newest written is the current plan.
    pub async fn mark_plan_written(&self, thread_id: &str, id: &str) -> Result<()> {
        // Past every other plan of the chat even within one millisecond, so the order of
        // writes is the order of the times.
        sqlx::query(
            "UPDATE sources SET plan_written_at = max(?, (
                 SELECT coalesce(max(plan_written_at), 0) + 1 FROM sources WHERE thread_id = ?))
             WHERE thread_id = ? AND source_id = ?",
        )
        .bind(timestamp_ms()?)
        .bind(thread_id)
        .bind(thread_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("mark the plan written")?;
        Ok(())
    }

    /// The chat's plans with when each was written, newest first.
    pub async fn plans(&self, thread_id: &str) -> Result<Vec<(Source, i64)>> {
        sqlx::query(
            "SELECT * FROM sources WHERE thread_id = ? AND plan_written_at IS NOT NULL
             ORDER BY plan_written_at DESC, created_at DESC, source_id DESC",
        )
        .bind(thread_id)
        .try_map(|row: SqliteRow| {
            let written: i64 = row.try_get("plan_written_at")?;
            Ok((source_from_row(row)?, written))
        })
        .fetch_all(&self.pool)
        .await
        .context("list the plans")
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
