//! The messages a user pinned in a thread, for the person alone: the model is never sent
//! them, and a pin keeps nothing from a compaction. A pin goes with its message.

use anyhow::{Context, Result, bail, ensure};
use sqlx::Row;

use super::{State, timestamp_ms};
use dusk_protocol::thread::{MAX_PINS, MessageRole, Pin, excerpt};

impl State {
    /// The thread's pins, oldest message first, each with its excerpt.
    pub async fn pins(&self, thread_id: &str) -> Result<Vec<Pin>> {
        let rows = sqlx::query(
            "SELECT p.message_id, p.pinned_at, m.sequence, m.role, m.content,
                m.sequence <= COALESCE((SELECT MAX(through_sequence) FROM compactions
                    WHERE thread_id = p.thread_id), 0) AS compacted
             FROM pinned_messages p JOIN messages m ON m.message_id = p.message_id
             WHERE p.thread_id = ? ORDER BY m.sequence",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .context("list the pins")?;
        rows.into_iter()
            .map(|row| {
                let role: String = row.try_get("role")?;
                let content: String = row.try_get("content")?;
                Ok(Pin {
                    message_id: row.try_get("message_id")?,
                    sequence: u64::try_from(row.try_get::<i64, _>("sequence")?)?,
                    role: match role.as_str() {
                        "user" => MessageRole::User,
                        _ => MessageRole::Assistant,
                    },
                    excerpt: excerpt(&content),
                    pinned_at: row.try_get("pinned_at")?,
                    compacted: row.try_get("compacted")?,
                })
            })
            .collect()
    }

    /// Pins or unpins a message of the thread; answers with the thread's pins. Only the
    /// user's and the agent's own words are pinned, at most `MAX_PINS` a thread.
    pub async fn set_pinned(
        &self,
        thread_id: &str,
        message_id: &str,
        pinned: bool,
    ) -> Result<Vec<Pin>> {
        let mut transaction = self.pool.begin().await.context("begin the pin")?;
        if pinned {
            let role: Option<String> = sqlx::query_scalar(
                "SELECT role FROM messages WHERE message_id = ? AND thread_id = ?
                    AND length(trim(content)) > 0",
            )
            .bind(message_id)
            .bind(thread_id)
            .fetch_optional(&mut *transaction)
            .await
            .context("find the message")?;
            match role.as_deref() {
                Some("user" | "assistant") => {}
                Some(_) => bail!("only your messages and the agent's answers are pinned"),
                None => bail!("no such message in this chat"),
            }
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM pinned_messages WHERE thread_id = ?")
                    .bind(thread_id)
                    .fetch_one(&mut *transaction)
                    .await?;
            let known: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM pinned_messages WHERE message_id = ?)",
            )
            .bind(message_id)
            .fetch_one(&mut *transaction)
            .await?;
            ensure!(known || count < MAX_PINS as i64, "Unpin one first");
            sqlx::query(
                "INSERT OR IGNORE INTO pinned_messages (message_id, thread_id, pinned_at)
                 VALUES (?, ?, ?)",
            )
            .bind(message_id)
            .bind(thread_id)
            .bind(timestamp_ms()?)
            .execute(&mut *transaction)
            .await
            .context("pin the message")?;
        } else {
            sqlx::query("DELETE FROM pinned_messages WHERE message_id = ? AND thread_id = ?")
                .bind(message_id)
                .bind(thread_id)
                .execute(&mut *transaction)
                .await
                .context("unpin the message")?;
        }
        transaction.commit().await.context("commit the pin")?;
        self.pins(thread_id).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        state::{TurnAnswer, TurnOutcome},
        testing::temp_state,
    };
    use dusk_protocol::thread::MAX_PINS;

    /// The cap, a message of another chat refused, and /undo taking its pins with it.
    #[tokio::test]
    async fn pins_are_capped_kept_to_their_chat_and_go_with_their_message() {
        let state = temp_state("pins").await;
        for thread in ["thread", "other"] {
            state.create_thread(thread, "test").await.unwrap();
        }
        let elsewhere = state.add_pending("other", "not here").await.unwrap();
        state.admit_pending("other", &elsewhere).await.unwrap();
        for n in 0..=MAX_PINS {
            let pending = state
                .add_pending("thread", &format!("question {n}"))
                .await
                .unwrap();
            state.admit_pending("thread", &pending).await.unwrap();
            let turn = state.start_turn("thread").await.unwrap().unwrap();
            let answer = TurnAnswer {
                content: format!("**answer** {n}"),
                reasoning: None,
            };
            let outcome = TurnOutcome::Completed(answer);
            state.finish_turn(&turn.turn_id, outcome).await.unwrap();
        }
        let messages = state.list_messages("thread").await.unwrap();
        for message in &messages[..MAX_PINS] {
            state
                .set_pinned("thread", &message.message_id, true)
                .await
                .unwrap();
        }
        let over = state
            .set_pinned("thread", &messages[MAX_PINS].message_id, true)
            .await;
        assert_eq!(over.unwrap_err().to_string(), "Unpin one first");
        let other = state.list_messages("other").await.unwrap();
        assert!(
            state
                .set_pinned("thread", &other[0].message_id, true)
                .await
                .is_err()
        );

        let pins = state.pins("thread").await.unwrap();
        assert_eq!(pins.len(), MAX_PINS);
        assert_eq!(pins[1].excerpt.as_str(), "answer 0");
        // Undo from the second question: the pins from there on go with their messages.
        state
            .undo_from("thread", messages[2].sequence)
            .await
            .unwrap();
        assert_eq!(state.pins("thread").await.unwrap().len(), 2);
        let pins = state
            .set_pinned("thread", &messages[0].message_id, false)
            .await
            .unwrap();
        assert_eq!(pins.len(), 1);
    }
}
