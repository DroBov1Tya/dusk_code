use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use crate::events::CoreEvent;

use super::State;

/// Mutable metadata. Permission modes and endpoint formats remain caller-defined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadInput {
    pub from: String,
    pub to: String,
    pub permission_mode: String,
    pub sandbox_enabled: bool,
    /// SQLite stores non-negative signed 64-bit integers; larger counts are rejected.
    pub token_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadMetadata {
    pub id: String,
    pub from: String,
    pub to: String,
    /// Unix timestamp in milliseconds, assigned by the core on creation.
    pub created_at: i64,
    /// Unix timestamp in milliseconds, non-decreasing across updates.
    pub updated_at: i64,
    pub permission_mode: String,
    pub sandbox_enabled: bool,
    pub token_count: u64,
}

impl State {
    /// Inserts a caller-assigned stable ID. Existing IDs are never overwritten.
    pub async fn create_thread(&self, id: &str, input: ThreadInput) -> Result<ThreadMetadata> {
        let token_count = validate_input(id, &input)?;
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin thread creation")?;
        let row = sqlx::query(
            r#"INSERT INTO threads
               (id, "from", "to", created_at, updated_at, permission_mode, sandbox_enabled, token_count)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING *"#,
        )
        .bind(id)
        .bind(input.from)
        .bind(input.to)
        .bind(timestamp)
        .bind(timestamp)
        .bind(input.permission_mode)
        .bind(input.sandbox_enabled)
        .bind(token_count)
        .fetch_one(&mut *transaction)
        .await
        .context("create thread metadata")?;
        let thread = decode_thread(row)?;
        transaction
            .commit()
            .await
            .context("commit thread creation")?;
        // No listeners is normal and does not invalidate a committed write.
        let _ = self.events.send(CoreEvent::ThreadCreated(thread.clone()));
        Ok(thread)
    }

    /// Replaces mutable metadata, preserving the stable ID and creation timestamp.
    /// Missing IDs are reported without creating a row or emitting an event.
    pub async fn update_thread(
        &self,
        id: &str,
        input: ThreadInput,
    ) -> Result<Option<ThreadMetadata>> {
        let token_count = validate_input(id, &input)?;
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin thread update")?;
        let row = sqlx::query(
            r#"UPDATE threads SET "from" = ?, "to" = ?,
               updated_at = MAX(updated_at, ?), permission_mode = ?,
               sandbox_enabled = ?, token_count = ? WHERE id = ? RETURNING *"#,
        )
        .bind(input.from)
        .bind(input.to)
        .bind(timestamp)
        .bind(input.permission_mode)
        .bind(input.sandbox_enabled)
        .bind(token_count)
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await
        .context("update thread metadata")?;
        let Some(row) = row else {
            return Ok(None);
        };
        let thread = decode_thread(row)?;
        transaction.commit().await.context("commit thread update")?;
        let _ = self.events.send(CoreEvent::ThreadUpdated(thread.clone()));
        Ok(Some(thread))
    }

    pub async fn load_thread(&self, id: &str) -> Result<Option<ThreadMetadata>> {
        sqlx::query("SELECT * FROM threads WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("load thread metadata")?
            .map(decode_thread)
            .transpose()
    }
}

fn validate_input(id: &str, input: &ThreadInput) -> Result<i64> {
    ensure!(!id.trim().is_empty(), "thread ID is empty");
    ensure!(!input.from.trim().is_empty(), "thread origin is empty");
    ensure!(!input.to.trim().is_empty(), "thread destination is empty");
    ensure!(
        !input.permission_mode.trim().is_empty(),
        "thread permission mode is empty"
    );
    i64::try_from(input.token_count).context("thread token count exceeds SQLite integer range")
}

fn timestamp_ms() -> Result<i64> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock precedes Unix epoch")?;
    i64::try_from(elapsed.as_millis()).context("timestamp exceeds SQLite integer range")
}

fn decode_thread(row: SqliteRow) -> Result<ThreadMetadata> {
    let token_count: i64 = row.try_get("token_count")?;
    Ok(ThreadMetadata {
        id: row.try_get("id")?,
        from: row.try_get("from")?,
        to: row.try_get("to")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        permission_mode: row.try_get("permission_mode")?,
        sandbox_enabled: row.try_get("sandbox_enabled")?,
        token_count: token_count
            .try_into()
            .context("negative thread token count")?,
    })
}
