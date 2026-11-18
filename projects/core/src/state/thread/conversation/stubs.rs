//! Stubs: short texts sent in place of old tool results, so they stop riding in every
//! request. The full result stays in `messages`. A stub is decided once and frozen: its
//! row holds the exact text sent, so the request stays the same bytes across loads and
//! restarts and the provider's cached prefix holds.

use std::collections::HashMap;

use anyhow::{Context, Result, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use super::{
    State,
    messages::{content_tokens, read_tokens},
};

/// A stub to store for a tool message of the thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewStub {
    pub message_id: String,
    /// The exact text sent in place of the result.
    pub text: String,
}

/// A stored stub, as the context sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stub {
    /// The exact text sent in place of the result.
    pub text: String,
    /// The message's size less the stub's, both as estimated when the stub was made.
    pub saved_tokens: u64,
    /// The newest message of the thread when the stub was made. A provider count of a
    /// request through an older message still holds the full result.
    pub made_at_sequence: u64,
}

impl State {
    /// Stores stubs made when the thread's newest message was `made_at_sequence`, all or
    /// none. Each must be for a tool message of this thread at or before that point, and
    /// one that has a stub already is refused: a stub is never rewritten, since that
    /// would change a request the provider has cached.
    pub async fn add_stubs(
        &self,
        thread_id: &str,
        made_at_sequence: u64,
        stubs: &[NewStub],
    ) -> Result<()> {
        let made_at = i64::try_from(made_at_sequence).context("sequence is too large")?;
        ensure!(made_at > 0, "a stub is made after a message");
        let mut transaction = self.pool.begin().await.context("begin stubs")?;
        for stub in stubs {
            ensure!(!stub.text.trim().is_empty(), "stub is empty");
            let row = sqlx::query(
                "SELECT m.content_tokens, s.message_id IS NOT NULL AS stubbed
                 FROM messages m LEFT JOIN result_stubs s ON s.message_id = m.message_id
                 WHERE m.message_id = ? AND m.thread_id = ? AND m.role = 'tool'
                    AND m.sequence <= ?",
            )
            .bind(&stub.message_id)
            .bind(thread_id)
            .bind(made_at)
            .fetch_optional(&mut *transaction)
            .await
            .context("read the stubbed message")?
            .with_context(|| {
                format!(
                    "message {} is not a tool result of this thread",
                    stub.message_id
                )
            })?;
            let stubbed: bool = row.try_get("stubbed")?;
            ensure!(
                !stubbed,
                "message {} has a stub already; a stub is never rewritten",
                stub.message_id
            );
            let saved =
                read_tokens(&row, "content_tokens")?.saturating_sub(content_tokens(&stub.text));
            sqlx::query(
                "INSERT INTO result_stubs (message_id, thread_id, stub, saved_tokens,
                    made_at_sequence)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&stub.message_id)
            .bind(thread_id)
            .bind(&stub.text)
            .bind(i64::try_from(saved).context("stub is too large")?)
            .bind(made_at)
            .execute(&mut *transaction)
            .await
            .context("store a stub")?;
        }
        transaction.commit().await.context("commit stubs")
    }

    /// The stubs of the thread's messages after sequence `after`, by message ID: the
    /// ones a context that starts after a summary through `after` can send.
    pub async fn stubs(&self, thread_id: &str, after: u64) -> Result<HashMap<String, Stub>> {
        sqlx::query(
            "SELECT s.message_id, s.stub, s.saved_tokens, s.made_at_sequence
             FROM result_stubs s JOIN messages m ON m.message_id = s.message_id
             WHERE s.thread_id = ? AND m.sequence > ?",
        )
        .bind(thread_id)
        .bind(i64::try_from(after).context("sequence is too large")?)
        .fetch_all(&self.pool)
        .await
        .context("load stubs")?
        .into_iter()
        .map(decode_stub)
        .collect()
    }
}

fn decode_stub(row: SqliteRow) -> Result<(String, Stub)> {
    let stub = Stub {
        text: row.try_get("stub")?,
        saved_tokens: read_tokens(&row, "saved_tokens")?,
        made_at_sequence: read_tokens(&row, "made_at_sequence")?,
    };
    Ok((row.try_get("message_id")?, stub))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::NewStub;
    use crate::{
        providers::ToolCall,
        state::{Message, MessageRole, State, TurnAnswer, TurnOutcome},
        testing::temp_state,
    };

    /// A user message, a step with one call and its result, and the answer; returns
    /// the result.
    async fn step_with_result(state: &State, thread_id: &str, text: &str) -> Message {
        state
            .add_user_message(thread_id, &format!("{thread_id}-{text}"), "go")
            .await
            .unwrap();
        let turn = state.start_turn(thread_id).await.unwrap().unwrap();
        let call = ToolCall {
            id: format!("call-{text}"),
            name: "run_command".into(),
            arguments: json!({ "command": "cargo test" }),
        };
        let called = TurnAnswer {
            content: String::new(),
            reasoning: None,
        };
        let id = &turn.turn_id;
        state
            .record_tool_calls(id, called, vec![call])
            .await
            .unwrap();
        state
            .record_tool_result(id, &format!("call-{text}"), text.into())
            .await
            .unwrap();
        let done = TurnAnswer {
            content: "done".into(),
            reasoning: None,
        };
        state
            .finish_turn(id, TurnOutcome::Completed(done))
            .await
            .unwrap();
        let messages = state.list_messages(thread_id).await.unwrap();
        messages
            .into_iter()
            .find(|message| message.role == MessageRole::Tool && message.content == text)
            .unwrap()
    }

    fn stub(message: &Message, text: &str) -> NewStub {
        NewStub {
            message_id: message.message_id.clone(),
            text: text.into(),
        }
    }

    #[tokio::test]
    async fn a_stub_is_decided_once() {
        let state = temp_state("stubs-once").await;
        state.create_thread("thread", "test").await.unwrap();
        let result = step_with_result(&state, "thread", &"x".repeat(4000)).await;

        state
            .add_stubs("thread", 4, &[stub(&result, "[result put away]")])
            .await
            .unwrap();
        let again = state
            .add_stubs("thread", 4, &[stub(&result, "[another text]")])
            .await;

        assert!(again.is_err());
        let stubs = state.stubs("thread", 0).await.unwrap();
        let stored = &stubs[&result.message_id];
        assert_eq!(stored.text, "[result put away]");
        assert_eq!(stored.made_at_sequence, 4);
        let sent = super::content_tokens("[result put away]");
        assert_eq!(stored.saved_tokens, result.content_tokens - sent);
    }

    #[tokio::test]
    async fn only_a_tool_result_of_the_thread_before_the_point_takes_a_stub() {
        let state = temp_state("stubs-refused").await;
        for thread in ["thread", "other"] {
            state.create_thread(thread, "test").await.unwrap();
        }
        let result = step_with_result(&state, "thread", "output").await;
        let messages = state.list_messages("thread").await.unwrap();
        let user = messages
            .iter()
            .find(|message| message.role == MessageRole::User)
            .unwrap();
        let fine = stub(&result, "[put away]");
        let refused = [
            ("other", 4, vec![fine.clone()]),
            ("thread", 4, vec![stub(user, "[put away]")]),
            ("thread", result.sequence - 1, vec![fine.clone()]),
            ("thread", 0, vec![fine.clone()]),
            ("thread", 4, vec![stub(&result, " \n")]),
            ("thread", 4, vec![stub(&result, "[put away]"), fine.clone()]),
        ];
        for (thread, at, stubs) in refused {
            assert!(
                state.add_stubs(thread, at, &stubs).await.is_err(),
                "{stubs:?}"
            );
        }
        // All or none: the batch with a repeat left nothing behind.
        assert!(state.stubs("thread", 0).await.unwrap().is_empty());
        let id = "x' OR 1=1 --";
        let odd = NewStub {
            message_id: id.into(),
            text: "[put away]".into(),
        };
        assert!(state.add_stubs("thread", 4, &[odd]).await.is_err());
    }

    #[tokio::test]
    async fn the_stubs_of_a_context_start_after_its_summary() {
        let state = temp_state("stubs-after").await;
        state.create_thread("thread", "test").await.unwrap();
        let first = step_with_result(&state, "thread", "first").await;
        let second = step_with_result(&state, "thread", "second").await;
        let stubs = [stub(&first, "[first]"), stub(&second, "[second]")];
        state.add_stubs("thread", 8, &stubs).await.unwrap();

        let after = state.stubs("thread", first.sequence).await.unwrap();

        assert_eq!(after.len(), 1);
        assert_eq!(after[&second.message_id].text, "[second]");
        assert!(state.stubs("other", 0).await.unwrap().is_empty());
    }

    /// `/undo` deletes messages; their stubs go with them.
    #[tokio::test]
    async fn a_stub_goes_with_its_message_on_undo() {
        let state = temp_state("stubs-undo").await;
        state.create_thread("thread", "test").await.unwrap();
        let first = step_with_result(&state, "thread", "first").await;
        let second = step_with_result(&state, "thread", "second").await;
        let stubs = [stub(&first, "[first]"), stub(&second, "[second]")];
        state.add_stubs("thread", 8, &stubs).await.unwrap();
        let points = state.undo_points("thread").await.unwrap();

        state.undo_from("thread", points[1].sequence).await.unwrap();

        let left = state.stubs("thread", 0).await.unwrap();
        assert_eq!(left.keys().collect::<Vec<_>>(), [&first.message_id]);
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM result_stubs")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(rows, 1);
        state.delete_thread("thread").await.unwrap();
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM result_stubs")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(rows, 0, "a deleted thread's stubs go with it");
    }
}
