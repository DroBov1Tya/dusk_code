//! Shaped results: a command's output as the model is sent it, shorter than the stored
//! result (`limits.shape_output`). Like a stub, the row holds the exact text sent and is
//! never rewritten, so requests stay the same bytes; unlike one, it is made with its
//! result, and a stub may still stand in for both later.

use std::collections::HashMap;

use anyhow::{Context, Result, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use super::{State, messages::read_tokens, stubs::saved_tokens};

/// A stored shaped result, as the context sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapedResult {
    /// The exact text sent in place of the result.
    pub text: String,
    /// The message's size less the text's, both as estimated when it was stored.
    pub saved_tokens: u64,
}

impl State {
    /// Stores `text` to be sent in place of tool message `message_id` of the thread, and
    /// says how many tokens it saves. A message shaped already is refused: the text is
    /// never rewritten, since that would change a request the provider has cached.
    pub async fn add_shaped(&self, thread_id: &str, message_id: &str, text: &str) -> Result<u64> {
        ensure!(!text.trim().is_empty(), "shaped text is empty");
        let mut transaction = self.pool.begin().await.context("begin shaped result")?;
        let row = sqlx::query(
            "SELECT m.content_tokens, s.message_id IS NOT NULL AS shaped
             FROM messages m LEFT JOIN shaped_results s ON s.message_id = m.message_id
             WHERE m.message_id = ? AND m.thread_id = ? AND m.role = 'tool'",
        )
        .bind(message_id)
        .bind(thread_id)
        .fetch_optional(&mut *transaction)
        .await
        .context("read the shaped message")?
        .with_context(|| format!("message {message_id} is not a tool result of this thread"))?;
        let shaped: bool = row.try_get("shaped")?;
        ensure!(!shaped, "message {message_id} is shaped already");
        let saved = saved_tokens(read_tokens(&row, "content_tokens")?, text);
        sqlx::query(
            "INSERT INTO shaped_results (message_id, thread_id, text, saved_tokens)
             VALUES (?, ?, ?, ?)",
        )
        .bind(message_id)
        .bind(thread_id)
        .bind(text)
        .bind(i64::try_from(saved).context("shaped result is too large")?)
        .execute(&mut *transaction)
        .await
        .context("store a shaped result")?;
        transaction.commit().await.context("commit shaped result")?;
        Ok(saved)
    }

    /// The shaped results of the thread's messages after sequence `after`, by message ID.
    pub async fn shaped_results(
        &self,
        thread_id: &str,
        after: u64,
    ) -> Result<HashMap<String, ShapedResult>> {
        sqlx::query(
            "SELECT s.message_id, s.text, s.saved_tokens
             FROM shaped_results s JOIN messages m ON m.message_id = s.message_id
             WHERE s.thread_id = ? AND m.sequence > ?",
        )
        .bind(thread_id)
        .bind(i64::try_from(after).context("sequence is too large")?)
        .fetch_all(&self.pool)
        .await
        .context("load shaped results")?
        .into_iter()
        .map(decode)
        .collect()
    }
}

fn decode(row: SqliteRow) -> Result<(String, ShapedResult)> {
    let shaped = ShapedResult {
        text: row.try_get("text")?,
        saved_tokens: read_tokens(&row, "saved_tokens")?,
    };
    Ok((row.try_get("message_id")?, shaped))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{
        providers::ToolCall,
        state::{MessageRole, TurnAnswer},
        testing::temp_state,
    };

    #[tokio::test]
    async fn a_shaped_result_is_decided_once_for_a_tool_result_of_its_thread() {
        let state = temp_state("shaped-once").await;
        for thread in ["thread", "other"] {
            state.create_thread(thread, "test").await.unwrap();
        }
        state.add_user_message("thread", "u", "go").await.unwrap();
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        let call = ToolCall {
            id: "c".into(),
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
        let result = state
            .record_tool_result(id, "c", "x".repeat(4000))
            .await
            .unwrap();
        let user = state.list_messages("thread").await.unwrap();
        let user = user.iter().find(|m| m.role == MessageRole::User).unwrap();

        for (thread, message, text) in [
            ("other", &result.message_id, "short"),
            ("thread", &user.message_id, "short"),
            ("thread", &result.message_id, " \n"),
            ("thread", &"x' OR 1=1 --".to_owned(), "short"),
        ] {
            assert!(state.add_shaped(thread, message, text).await.is_err());
        }
        let saved = state
            .add_shaped("thread", &result.message_id, "short")
            .await
            .unwrap();
        // "short" is 2 tokens, and 4 of markup.
        assert_eq!(saved, result.content_tokens - 6);
        let again = state
            .add_shaped("thread", &result.message_id, "other")
            .await;
        assert!(again.is_err());
        let stored = state.shaped_results("thread", 0).await.unwrap();
        assert_eq!(stored[&result.message_id].text, "short");
        assert!(
            state
                .shaped_results("thread", result.sequence)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(state.shaped_results("other", 0).await.unwrap().is_empty());
        state.delete_thread("thread").await.unwrap();
        let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM shaped_results")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(rows, 0, "a deleted thread's shaped results go with it");
    }
}
