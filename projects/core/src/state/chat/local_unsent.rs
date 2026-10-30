//! Durable client messages that have not been offered to any provider.

use anyhow::{Context, Result, ensure};
use sqlx::Row;
use uuid::Uuid;

use super::{State, timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalUnsent {
    pub id: String,
    pub thread_id: String,
    pub content: String,
    pub created_at: i64,
}

impl State {
    pub async fn add_local_unsent(&self, thread_id: &str, content: &str) -> Result<LocalUnsent> {
        ensure!(!content.trim().is_empty(), "message is empty");
        let item = LocalUnsent {
            id: Uuid::now_v7().to_string(),
            thread_id: thread_id.to_owned(),
            content: content.to_owned(),
            created_at: timestamp_ms()?,
        };
        sqlx::query(
            "INSERT INTO local_unsent (id, thread_id, content, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&item.id)
        .bind(&item.thread_id)
        .bind(&item.content)
        .bind(item.created_at)
        .execute(&self.pool)
        .await
        .context("store local unsent message")?;
        Ok(item)
    }

    pub async fn local_unsent(&self, thread_id: &str) -> Result<Vec<LocalUnsent>> {
        let rows = sqlx::query("SELECT id, thread_id, content, created_at FROM local_unsent WHERE thread_id = ? ORDER BY created_at, id")
            .bind(thread_id).fetch_all(&self.pool).await.context("list local unsent messages")?;
        rows.into_iter()
            .map(|row| {
                Ok(LocalUnsent {
                    id: row.try_get("id")?,
                    thread_id: row.try_get("thread_id")?,
                    content: row.try_get("content")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }

    /// The only transition into the runner queue. One SQLite transaction prevents a
    /// crash from losing or duplicating the message between the two tables.
    pub async fn promote_local_unsent(&self, thread_id: &str, id: &str) -> Result<()> {
        let mut transaction = self.pool.begin().await.context("begin local resend")?;
        let row = sqlx::query("SELECT content FROM local_unsent WHERE id = ? AND thread_id = ?")
            .bind(id)
            .bind(thread_id)
            .fetch_optional(&mut *transaction)
            .await
            .context("read local unsent message")?;
        let Some(row) = row else {
            let already_sent: i64 = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pending_messages WHERE thread_id = ? AND request_uid = ?)
                 OR EXISTS(SELECT 1 FROM messages WHERE thread_id = ? AND request_uid = ?)")
                .bind(thread_id).bind(id).bind(thread_id).bind(id)
                .fetch_one(&mut *transaction).await.context("check local resend")?;
            ensure!(already_sent != 0, "local unsent message does not exist");
            return Ok(());
        };
        let content: String = row.try_get("content")?;
        sqlx::query(
            "INSERT INTO pending_messages (thread_id, request_uid, content, created_at, from_thread_id)
             VALUES (?, ?, ?, ?, NULL)")
            .bind(thread_id).bind(id).bind(&content).bind(timestamp_ms()?)
            .execute(&mut *transaction).await.context("queue local resend")?;
        sqlx::query("DELETE FROM local_unsent WHERE id = ? AND thread_id = ?")
            .bind(id)
            .bind(thread_id)
            .execute(&mut *transaction)
            .await
            .context("remove promoted local message")?;
        transaction.commit().await.context("commit local resend")?;
        Ok(())
    }
}
