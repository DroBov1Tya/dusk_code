//! Messages a client or another agent sent that the thread has not taken in yet.

use anyhow::{Context, Result, ensure};
use sqlx::Row;
use uuid::Uuid;

use super::{Message, State, timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMessage {
    pub pending_id: i64,
    pub request_uid: String,
    pub content: String,
    /// The agent's thread for a message another agent sent; `None` for a client's.
    pub from_thread_id: Option<String>,
    pub from_job_id: Option<String>,
}

impl State {
    /// Stores what a client sent; once this returns, the message survives a crash.
    pub async fn add_pending(&self, thread_id: &str, content: &str) -> Result<PendingMessage> {
        self.add_pending_from(thread_id, content, None).await
    }

    /// The same for a message another agent sent; `from` is that agent's thread.
    pub async fn add_pending_from(
        &self,
        thread_id: &str,
        content: &str,
        from: Option<&str>,
    ) -> Result<PendingMessage> {
        ensure!(!content.trim().is_empty(), "message is empty");
        let request_uid = Uuid::now_v7().to_string();
        let pending_id = sqlx::query_scalar(
            "INSERT INTO pending_messages (thread_id, request_uid, content, created_at,
                from_thread_id)
             VALUES (?, ?, ?, ?, ?) RETURNING pending_id",
        )
        .bind(thread_id)
        .bind(&request_uid)
        .bind(content)
        .bind(timestamp_ms()?)
        .bind(from)
        .fetch_one(&self.pool)
        .await
        .context("store sent message")?;
        Ok(PendingMessage {
            pending_id,
            request_uid,
            content: content.to_owned(),
            from_thread_id: from.map(str::to_owned),
            from_job_id: None,
        })
    }

    /// Drops queued requests; runtime-owned job results remain for the next resume.
    pub async fn drop_pending(&self, thread_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM pending_messages WHERE thread_id = ? AND from_job_id IS NULL")
            .bind(thread_id)
            .execute(&self.pool)
            .await
            .context("drop waiting messages")?;
        Ok(())
    }

    /// Threads with messages waiting to be read.
    pub async fn threads_with_pending(&self) -> Result<Vec<String>> {
        sqlx::query_scalar("SELECT DISTINCT thread_id FROM pending_messages")
            .fetch_all(&self.pool)
            .await
            .context("list threads with waiting messages")
    }

    pub async fn has_pending(&self, thread_id: &str) -> Result<bool> {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pending_messages WHERE thread_id = ?)")
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await
            .context("look for waiting messages")
    }

    pub async fn pending_messages(&self, thread_id: &str) -> Result<Vec<PendingMessage>> {
        let rows = sqlx::query(
            "SELECT pending_id, request_uid, content, from_thread_id, from_job_id FROM pending_messages
             WHERE thread_id = ? ORDER BY pending_id",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .context("list sent messages")?;
        rows.into_iter()
            .map(|row| {
                Ok(PendingMessage {
                    pending_id: row.try_get("pending_id")?,
                    request_uid: row.try_get("request_uid")?,
                    content: row.try_get("content")?,
                    from_thread_id: row.try_get("from_thread_id")?,
                    from_job_id: row.try_get("from_job_id")?,
                })
            })
            .collect()
    }

    /// Moves a sent message into the thread. A crash between the two writes is safe: the
    /// request UID makes the next attempt return the stored message.
    pub async fn admit_pending(
        &self,
        thread_id: &str,
        pending: &PendingMessage,
    ) -> Result<Message> {
        let from = pending.from_thread_id.as_deref();
        let message = match &pending.from_job_id {
            Some(job) => self.admit_job(thread_id, job, &pending.content).await?,
            None => {
                self.add_message_from(thread_id, &pending.request_uid, &pending.content, from)
                    .await?
            }
        };
        self.remove_pending(pending.pending_id).await?;
        Ok(message)
    }

    /// For a sent message that was not a message after all, such as a command.
    pub async fn remove_pending(&self, pending_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM pending_messages WHERE pending_id = ?")
            .bind(pending_id)
            .execute(&self.pool)
            .await
            .context("remove sent message")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::temp_state;

    #[tokio::test]
    async fn admitting_twice_adds_the_message_once() {
        let state = temp_state("pending_twice").await;
        state.create_thread("thread", "test").await.unwrap();
        let pending = state.add_pending("thread", "hello").await.unwrap();

        let first = state.admit_pending("thread", &pending).await.unwrap();
        let again = state.admit_pending("thread", &pending).await.unwrap();

        assert_eq!(first, again);
        assert!(state.pending_messages("thread").await.unwrap().is_empty());
        assert_eq!(state.list_messages("thread").await.unwrap().len(), 1);
    }
}
