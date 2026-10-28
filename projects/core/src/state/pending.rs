//! Messages a client sent that the thread has not taken in yet.

use anyhow::{Context, Result, ensure};
use sqlx::Row;
use uuid::Uuid;

use super::{Message, State, timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMessage {
    pub pending_id: i64,
    pub request_uid: String,
    pub content: String,
}

impl State {
    /// Stores what a client sent; once this returns, the message survives a crash.
    pub async fn add_pending(&self, thread_id: &str, content: &str) -> Result<PendingMessage> {
        ensure!(!content.trim().is_empty(), "message is empty");
        let request_uid = Uuid::now_v7().to_string();
        let pending_id = sqlx::query_scalar(
            "INSERT INTO pending_messages (thread_id, request_uid, content, created_at)
             VALUES (?, ?, ?, ?) RETURNING pending_id",
        )
        .bind(thread_id)
        .bind(&request_uid)
        .bind(content)
        .bind(timestamp_ms()?)
        .fetch_one(&self.pool)
        .await
        .context("store sent message")?;
        Ok(PendingMessage {
            pending_id,
            request_uid,
            content: content.to_owned(),
        })
    }

    /// Oldest first.
    pub async fn pending_messages(&self, thread_id: &str) -> Result<Vec<PendingMessage>> {
        let rows = sqlx::query(
            "SELECT pending_id, request_uid, content FROM pending_messages
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
        let message = self
            .add_user_message(thread_id, &pending.request_uid, &pending.content)
            .await?;
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
