//! Messages a client or another agent sent that the thread has not taken in yet.

use anyhow::{Context, Result, ensure};
use sqlx::Row;
use uuid::Uuid;

use super::{Message, State, messages, timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMessage {
    pub pending_id: i64,
    pub request_uid: String,
    pub content: String,
    /// The agent's thread for a message another agent sent; `None` for a client's.
    pub from_thread_id: Option<String>,
    pub from_job_id: Option<String>,
    /// When it was stored, in Unix ms.
    pub created_at: i64,
}

/// How far a message stored under a caller's `request_uid` got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Nothing was stored under the uid.
    None,
    /// Stored; its thread has not taken it in.
    Waiting,
    /// In the thread; `answered` once a turn that took it has ended.
    Read { answered: bool },
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
        let created_at = timestamp_ms()?;
        let pending_id = sqlx::query_scalar(
            "INSERT INTO pending_messages (thread_id, request_uid, content, created_at,
                from_thread_id)
             VALUES (?, ?, ?, ?, ?) RETURNING pending_id",
        )
        .bind(thread_id)
        .bind(&request_uid)
        .bind(content)
        .bind(created_at)
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
            created_at,
        })
    }

    /// Stores a message under the caller's `request_uid`, once: a sender that cannot know
    /// whether it stored the message before a crash stores it again, and the second time
    /// changes nothing, whether the first still waits or was taken into the thread.
    /// Answers whether it was stored now.
    pub async fn add_pending_once(
        &self,
        thread_id: &str,
        request_uid: &str,
        content: &str,
        from: Option<&str>,
    ) -> Result<bool> {
        ensure!(!request_uid.trim().is_empty(), "request UID is empty");
        ensure!(!content.trim().is_empty(), "message is empty");
        let stored = sqlx::query(
            "INSERT INTO pending_messages (thread_id, request_uid, content, created_at,
                from_thread_id)
             SELECT ?1, ?2, ?3, ?4, ?5
             WHERE NOT EXISTS (SELECT 1 FROM messages WHERE request_uid = ?2)
             ON CONFLICT (request_uid) DO NOTHING",
        )
        .bind(thread_id)
        .bind(request_uid)
        .bind(content)
        .bind(timestamp_ms()?)
        .bind(from)
        .execute(&self.pool)
        .await
        .context("store sent message once")?;
        Ok(stored.rows_affected() == 1)
    }

    /// Where the message stored under `request_uid` stands.
    pub async fn delivery(&self, request_uid: &str) -> Result<Delivery> {
        let read: Option<bool> = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM turns WHERE turns.thread_id = messages.thread_id
                 AND turns.last_sequence >= messages.sequence AND turns.status != 'running')
             FROM messages WHERE request_uid = ?",
        )
        .bind(request_uid)
        .fetch_optional(&self.pool)
        .await
        .context("look up a delivered message")?;
        if let Some(answered) = read {
            return Ok(Delivery::Read { answered });
        }
        let waits: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pending_messages WHERE request_uid = ?)",
        )
        .bind(request_uid)
        .fetch_one(&self.pool)
        .await
        .context("look up a waiting message")?;
        Ok(if waits {
            Delivery::Waiting
        } else {
            Delivery::None
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
            "SELECT pending_id, request_uid, content, from_thread_id, from_job_id, created_at
             FROM pending_messages
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
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }

    /// Moves a sent message into the thread, with its effect on the goal: another agent's
    /// message is new work, so the approved goal's proofs go stale; the user's message
    /// answers a goal that waits on them; a job's result changes nothing. One transaction
    /// stores the message and removes the pending row, so no reader sees it both waiting
    /// and read. Admitting it again returns the stored message, and the goal changes with
    /// the pending row's removal, so only once.
    pub async fn admit_pending(
        &self,
        thread_id: &str,
        pending: &PendingMessage,
    ) -> Result<Message> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .context("begin admitting a message")?;
        // The write comes first, so the transaction holds the write lock before it reads.
        let removed = sqlx::query("DELETE FROM pending_messages WHERE pending_id = ?")
            .bind(pending.pending_id)
            .execute(&mut *transaction)
            .await
            .context("remove sent message")?
            .rows_affected();
        let message = match &pending.from_job_id {
            Some(job) => {
                messages::job_message(&mut transaction, thread_id, job, &pending.content).await?
            }
            None => {
                messages::message_from(
                    &mut transaction,
                    thread_id,
                    &pending.request_uid,
                    &pending.content,
                    pending.from_thread_id.as_deref(),
                )
                .await?
            }
        };
        let goal = if removed == 0 || message.from_job_id.is_some() {
            None
        } else if message.from_thread_id.is_some() {
            Some(
                "UPDATE goals SET work_epoch = work_epoch + 1
                 WHERE thread_id = ? AND approved_at IS NOT NULL",
            )
        } else {
            Some("UPDATE goals SET awaiting_user = 0 WHERE thread_id = ?")
        };
        if let Some(goal) = goal {
            sqlx::query(goal)
                .bind(thread_id)
                .execute(&mut *transaction)
                .await
                .context("update the goal for a message")?;
        }
        transaction
            .commit()
            .await
            .context("commit admitting a message")?;
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

    /// Another agent's message makes an approved goal's proofs stale, once even if the
    /// message is admitted again; the user's message ends a goal's wait for them.
    #[tokio::test]
    async fn admitting_a_message_updates_the_goal_once() {
        let state = temp_state("pending_goal").await;
        state.create_thread("thread", "test").await.unwrap();
        state.create_thread("agent", "test").await.unwrap();
        state.set_goal("thread", "ship it").await.unwrap();
        sqlx::query("UPDATE goals SET approved_at = 1, awaiting_user = 1")
            .execute(&state.pool)
            .await
            .unwrap();
        let goal = || {
            sqlx::query_as::<_, (i64, bool)>(
                "SELECT work_epoch, awaiting_user FROM goals WHERE thread_id = 'thread'",
            )
            .fetch_one(&state.pool)
        };
        let (epoch, _) = goal().await.unwrap();

        let from_agent = state
            .add_pending_from("thread", "found it", Some("agent"))
            .await
            .unwrap();
        state.admit_pending("thread", &from_agent).await.unwrap();
        state.admit_pending("thread", &from_agent).await.unwrap();
        assert_eq!(goal().await.unwrap(), (epoch + 1, true));

        let from_user = state.add_pending("thread", "go on").await.unwrap();
        state.admit_pending("thread", &from_user).await.unwrap();
        assert_eq!(goal().await.unwrap(), (epoch + 1, false));
    }

    #[tokio::test]
    async fn a_message_stored_under_the_callers_uid_is_stored_once() {
        use super::Delivery;

        let state = temp_state("pending_once").await;
        state.create_thread("thread", "test").await.unwrap();
        assert_eq!(state.delivery("card:t:1").await.unwrap(), Delivery::None);

        let once =
            |content: &'static str| state.add_pending_once("thread", "card:t:1", content, None);
        assert!(once("the card").await.unwrap());
        // Again while it waits, with other words: the first stands.
        assert!(!once("another card").await.unwrap());
        assert_eq!(state.delivery("card:t:1").await.unwrap(), Delivery::Waiting);
        let waiting = state.pending_messages("thread").await.unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].content, "the card");

        // Again after the thread took it in: nothing waits a second time.
        state.admit_pending("thread", &waiting[0]).await.unwrap();
        assert!(!once("the card").await.unwrap());
        assert!(state.pending_messages("thread").await.unwrap().is_empty());
        let read = Delivery::Read { answered: false };
        assert_eq!(state.delivery("card:t:1").await.unwrap(), read);
        // A turn that took it and ended has answered it; a running one has not.
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        assert_eq!(state.delivery("card:t:1").await.unwrap(), read);
        state.interrupt_running_turns("thread").await.unwrap();
        let answered = Delivery::Read { answered: true };
        assert_eq!(
            state.delivery("card:t:1").await.unwrap(),
            answered,
            "{turn:?}"
        );
        assert!(once(" ").await.is_err());
        assert!(
            state
                .add_pending_once("thread", " ", "x", None)
                .await
                .is_err()
        );
    }
}
