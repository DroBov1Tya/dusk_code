//! How many tokens a thread holds and what its model calls cost. Sizes are estimated per
//! message, so they split by who wrote what; costs are what the provider reported.

use anyhow::{Context, Result};
use sqlx::Row;

use super::{State, messages::read_tokens, timestamp_ms};
use crate::providers::Usage;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThreadTokens {
    /// What the model receives now: the messages after the thread's compaction point.
    pub current: TokenCounts,
    /// Every message the thread ever had, compacted ones included.
    pub all: TokenCounts,
    /// Sums of what the provider reported over every model call of the thread.
    pub spent: Usage,
    pub model_calls: u64,
}

/// Estimated tokens by who wrote them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenCounts {
    /// MAIN.md and other system instructions.
    pub system: u64,
    /// What the client sent.
    pub user: u64,
    /// The agent's answers.
    pub assistant: u64,
    /// The agent's thinking; never sent back, so it is always 0 in the current context.
    pub reasoning: u64,
    pub tool_calls: u64,
    pub tool_results: u64,
}

impl TokenCounts {
    pub fn total(&self) -> u64 {
        self.system
            + self.user
            + self.assistant
            + self.reasoning
            + self.tool_calls
            + self.tool_results
    }

    fn add(&mut self, role: &str, content: u64, tool_calls: u64, reasoning: u64) {
        match role {
            "system" => self.system += content,
            "user" => self.user += content,
            "assistant" => self.assistant += content,
            _ => self.tool_results += content,
        }
        self.tool_calls += tool_calls;
        self.reasoning += reasoning;
    }
}

impl State {
    /// Stores the usage the provider reported for one model call of a turn.
    pub async fn record_usage(&self, turn_id: &str, model: &str, usage: Usage) -> Result<()> {
        let timestamp = timestamp_ms()?;
        sqlx::query(
            "INSERT INTO model_usage (turn_id, model, prompt_tokens, cached_prompt_tokens,
                completion_tokens, reasoning_tokens, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(turn_id)
        .bind(model)
        .bind(sql_count(usage.prompt_tokens)?)
        .bind(sql_count(usage.cached_prompt_tokens)?)
        .bind(sql_count(usage.completion_tokens)?)
        .bind(sql_count(usage.reasoning_tokens)?)
        .bind(timestamp)
        .execute(&self.pool)
        .await
        .context("store model usage")?;
        Ok(())
    }

    pub async fn thread_tokens(&self, thread_id: &str) -> Result<ThreadTokens> {
        let mut tokens = ThreadTokens::default();
        let groups = sqlx::query(
            "SELECT role, sequence > threads.context_from AS in_context,
                SUM(content_tokens) AS content_tokens,
                SUM(tool_calls_tokens) AS tool_calls_tokens,
                SUM(reasoning_tokens) AS reasoning_tokens
             FROM messages JOIN threads ON threads.id = messages.thread_id
             WHERE thread_id = ? GROUP BY role, in_context",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .context("count thread tokens")?;
        for group in groups {
            let role: String = group.try_get("role")?;
            let content = read_tokens(&group, "content_tokens")?;
            let tool_calls = read_tokens(&group, "tool_calls_tokens")?;
            let reasoning = read_tokens(&group, "reasoning_tokens")?;
            tokens.all.add(&role, content, tool_calls, reasoning);
            if group.try_get("in_context")? {
                tokens.current.add(&role, content, tool_calls, 0);
            }
        }

        let spent = sqlx::query(
            "SELECT COUNT(*) AS model_calls,
                COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens,
                COALESCE(SUM(cached_prompt_tokens), 0) AS cached_prompt_tokens,
                COALESCE(SUM(completion_tokens), 0) AS completion_tokens,
                COALESCE(SUM(model_usage.reasoning_tokens), 0) AS reasoning_tokens
             FROM model_usage JOIN turns USING (turn_id) WHERE thread_id = ?",
        )
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await
        .context("sum model usage")?;
        tokens.model_calls = read_tokens(&spent, "model_calls")?;
        tokens.spent = Usage {
            prompt_tokens: read_tokens(&spent, "prompt_tokens")?,
            cached_prompt_tokens: read_tokens(&spent, "cached_prompt_tokens")?,
            completion_tokens: read_tokens(&spent, "completion_tokens")?,
            reasoning_tokens: read_tokens(&spent, "reasoning_tokens")?,
        };
        Ok(tokens)
    }
}

fn sql_count(count: u64) -> Result<i64> {
    i64::try_from(count).context("token count exceeds SQLite integer range")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        providers::ToolCall,
        state::messages::estimate_tokens,
        state::{StepResult, TurnAnswer, TurnOutcome, TurnStep},
        testing::temp_state,
    };

    /// A system prompt, a question, a tool step with reasoning and a final answer.
    async fn thread_with_a_tool_step(name: &str) -> (State, String) {
        let state = temp_state(name).await;
        state.create_thread("thread", "test").await.unwrap();
        state.add_system_message("thread", "1234").await.unwrap();
        state
            .add_user_message("thread", "uid", "12345678")
            .await
            .unwrap();
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        let step = TurnStep {
            answer: TurnAnswer {
                content: String::new(),
                reasoning: Some("1234".into()),
            },
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "ping".into(),
                arguments: json!({}),
            }],
            results: vec![StepResult {
                call_id: "call_1".into(),
                content: "pong".into(),
            }],
        };
        state.record_step(&turn.turn_id, step).await.unwrap();
        let answer = TurnAnswer {
            content: "1234".into(),
            reasoning: None,
        };
        let outcome = TurnOutcome::Completed(answer);
        state.finish_turn(&turn.turn_id, outcome).await.unwrap();
        (state, turn.turn_id)
    }

    #[tokio::test]
    async fn splits_tokens_by_who_wrote_them() {
        let (state, _) = thread_with_a_tool_step("tokens_split").await;

        let tokens = state.thread_tokens("thread").await.unwrap();

        // Every message adds 4 tokens of role markup; the tool step's empty text too.
        let calls = estimate_tokens(r#"[{"id":"call_1","name":"ping","arguments":{}}]"#);
        let expected = TokenCounts {
            system: 5,
            user: 6,
            assistant: 4 + 5,
            reasoning: 0,
            tool_calls: calls,
            tool_results: 5,
        };
        assert_eq!(tokens.current, expected);
        assert_eq!(
            tokens.all,
            TokenCounts {
                reasoning: 1,
                ..expected
            }
        );
        assert_eq!(tokens.model_calls, 0);
    }

    #[tokio::test]
    async fn compacted_messages_leave_the_context_but_stay_in_history() {
        let (state, _) = thread_with_a_tool_step("tokens_compacted").await;
        sqlx::query("UPDATE threads SET context_from = 2")
            .execute(&state.pool)
            .await
            .unwrap();

        let tokens = state.thread_tokens("thread").await.unwrap();

        assert_eq!(tokens.current.system, 0);
        assert_eq!(tokens.current.user, 0);
        assert_eq!(tokens.all.system, 5);
        assert_eq!(tokens.current.total() + 11, tokens.all.total() - 1);
    }

    #[tokio::test]
    async fn sums_what_the_provider_reported() {
        let (state, turn_id) = thread_with_a_tool_step("tokens_usage").await;
        let usage = Usage {
            prompt_tokens: 100,
            cached_prompt_tokens: 60,
            completion_tokens: 20,
            reasoning_tokens: 5,
        };
        state.record_usage(&turn_id, "model", usage).await.unwrap();
        state.record_usage(&turn_id, "model", usage).await.unwrap();

        let tokens = state.thread_tokens("thread").await.unwrap();

        assert_eq!(tokens.model_calls, 2);
        assert_eq!(
            tokens.spent,
            Usage {
                prompt_tokens: 200,
                cached_prompt_tokens: 120,
                completion_tokens: 40,
                reasoning_tokens: 10,
            }
        );
    }
}
