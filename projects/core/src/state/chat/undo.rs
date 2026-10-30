//! Taking a thread back to before one of the user's messages: that message and all
//! after it go, with the turns that answered them, the compactions that summed them up,
//! and the usage of the model calls that read them. The search index is kept in step,
//! since it reads its text from the messages.

use anyhow::{Context, Result, ensure};
use sqlx::Row;

use super::State;
use crate::state::MessageRole;

/// A user message the thread can go back to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoPoint {
    pub sequence: u64,
    pub text: String,
}

impl State {
    /// The user's own messages, oldest first; messages from agents are not points.
    pub async fn undo_points(&self, thread_id: &str) -> Result<Vec<UndoPoint>> {
        let messages = self.list_messages(thread_id).await?;
        Ok(messages
            .into_iter()
            .filter(|message| message.role == MessageRole::User && message.from_thread_id.is_none())
            .map(|message| UndoPoint {
                sequence: message.sequence,
                text: message.content,
            })
            .collect())
    }

    /// Removes the user message at `sequence` and everything after it; returns its text.
    /// The thread must not be in a turn.
    pub async fn undo_from(&self, thread_id: &str, sequence: u64) -> Result<String> {
        let sequence = i64::try_from(sequence).context("sequence is too large")?;
        let mut transaction = self.pool.begin().await.context("begin undo")?;
        let row = sqlx::query(
            "SELECT content FROM messages WHERE thread_id = ? AND sequence = ?
                AND role = 'user' AND from_thread_id IS NULL",
        )
        .bind(thread_id)
        .bind(sequence)
        .fetch_optional(&mut *transaction)
        .await
        .context("read the undo point")?;
        let row = row.context("no message of yours at that point")?;
        let text: String = row.try_get("content")?;
        let running: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM turns WHERE thread_id = ? AND status = 'running')",
        )
        .bind(thread_id)
        .fetch_one(&mut *transaction)
        .await
        .context("look for a running turn")?;
        ensure!(!running, "the thread is in a turn; stop it first");
        // Later rows point at earlier ones, so they go first.
        let steps = [
            "DELETE FROM model_usage WHERE thread_id = ?1 AND (through_sequence >= ?2
                OR turn_id IN (SELECT turn_id FROM turns WHERE thread_id = ?1
                    AND (last_sequence >= ?2 OR response_message_id IN
                        (SELECT message_id FROM messages WHERE thread_id = ?1 AND sequence >= ?2)))
                OR compaction_id IN (SELECT compaction_id FROM compactions
                    WHERE thread_id = ?1 AND through_sequence >= ?2))",
            "DELETE FROM turns WHERE thread_id = ?1 AND (last_sequence >= ?2
                OR response_message_id IN
                    (SELECT message_id FROM messages WHERE thread_id = ?1 AND sequence >= ?2))",
            "DELETE FROM compactions WHERE thread_id = ?1 AND through_sequence >= ?2",
            "INSERT INTO messages_search (messages_search, rowid, content, tool_calls_json)
             SELECT 'delete', rowid, content, tool_calls_json FROM messages
             WHERE thread_id = ?1 AND sequence >= ?2",
            "DELETE FROM messages WHERE thread_id = ?1 AND sequence >= ?2",
        ];
        for step in steps {
            sqlx::query(step)
                .bind(thread_id)
                .bind(sequence)
                .execute(&mut *transaction)
                .await
                .context("undo")?;
        }
        transaction.commit().await.context("commit undo")?;
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        state::{State, TurnAnswer, TurnOutcome},
        testing::temp_state,
    };

    async fn exchange(state: &State, question: &str, answer: &str) {
        let pending = state.add_pending("thread", question).await.unwrap();
        state.admit_pending("thread", &pending).await.unwrap();
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        let answer = TurnAnswer {
            content: answer.into(),
            reasoning: None,
        };
        let outcome = TurnOutcome::Completed(answer);
        state.finish_turn(&turn.turn_id, outcome).await.unwrap();
    }

    #[tokio::test]
    async fn undo_removes_the_message_and_all_after_it() {
        let state = temp_state("undo").await;
        state.create_thread("thread", "test").await.unwrap();
        exchange(&state, "first question", "first answer").await;
        exchange(&state, "second question", "second answer").await;
        state.add_compaction("thread", 4, "both").await.unwrap();
        let points = state.undo_points("thread").await.unwrap();
        assert_eq!(points.len(), 2);

        let text = state.undo_from("thread", points[1].sequence).await.unwrap();

        assert_eq!(text, "second question");
        let left: Vec<_> = state.list_messages("thread").await.unwrap();
        assert_eq!(left.len(), 2);
        let found = state.search_messages("thread", "second", 10).await.unwrap();
        assert!(found.is_empty(), "the index forgot them");
        assert!(state.latest_compaction("thread").await.unwrap().is_none());
        // The thread goes on as before.
        exchange(&state, "third question", "third answer").await;
        assert_eq!(state.list_messages("thread").await.unwrap().len(), 4);
        assert!(
            state.undo_from("thread", 2).await.is_err(),
            "an answer is no point"
        );
    }
}
