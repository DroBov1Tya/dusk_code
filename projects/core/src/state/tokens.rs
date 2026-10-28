//! How many tokens a thread holds and what its model calls cost. Sizes are estimated per
//! message, so they split by who wrote what; costs are what the provider reported.

use anyhow::{Context, Result};
use sqlx::Row;

use super::{State, messages::read_tokens, timestamp_ms};

use crate::providers::Usage;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThreadTokens {
    /// The best known size of the current context: what the provider counted for the
    /// last call plus estimates of what came after it. Before any call, `current.total()`.
    pub context: u64,
    /// What the model receives now: the system messages, the newest summary and the
    /// messages after it.
    pub current: TokenCounts,
    /// Every message and summary the thread ever had.
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
    /// Compaction summaries, which stand in for the older messages.
    pub summary: u64,
    /// Tool definitions, sent with every request but never stored, so the agent counts
    /// them into the current context only.
    pub tool_specs: u64,
}

impl TokenCounts {
    pub fn total(&self) -> u64 {
        self.system
            + self.user
            + self.assistant
            + self.reasoning
            + self.tool_calls
            + self.tool_results
            + self.summary
            + self.tool_specs
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

/// Which call a usage report belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCall {
    pub thread_id: String,
    /// `None` outside a turn, such as `/compact` while the thread is idle.
    pub turn_id: Option<String>,
    pub purpose: CallPurpose,
    /// The compaction the call's context started from.
    pub compaction_id: Option<i64>,
    pub model: String,
    /// The newest message the call sent.
    pub through_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPurpose {
    /// A step of the agent.
    Answer,
    /// A summary for a compaction.
    Compact,
    /// A name for the thread.
    Title,
}

/// The provider's count of the newest request of a thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LastPrompt {
    pub prompt_tokens: u64,
    /// The newest message that request sent.
    pub through_sequence: u64,
}

impl State {
    pub async fn record_usage(&self, call: &ModelCall, usage: Usage) -> Result<()> {
        let timestamp = timestamp_ms()?;
        sqlx::query(
            "INSERT INTO model_usage (thread_id, turn_id, purpose, compaction_id, model,
                through_sequence, prompt_tokens, cached_prompt_tokens, completion_tokens,
                reasoning_tokens, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&call.thread_id)
        .bind(&call.turn_id)
        .bind(call.purpose.as_str())
        .bind(call.compaction_id)
        .bind(&call.model)
        .bind(sql_count(call.through_sequence)?)
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

    /// The newest agent step sent over the context that starts from `compaction_id`.
    /// Steps before a compaction sent other messages, so they do not count.
    pub async fn last_prompt(
        &self,
        thread_id: &str,
        compaction_id: Option<i64>,
    ) -> Result<Option<LastPrompt>> {
        let row = sqlx::query(
            "SELECT prompt_tokens, through_sequence FROM model_usage
             WHERE thread_id = ? AND purpose = 'answer' AND compaction_id IS ?
             ORDER BY created_at DESC, rowid DESC LIMIT 1",
        )
        .bind(thread_id)
        .bind(compaction_id)
        .fetch_optional(&self.pool)
        .await
        .context("load the last prompt size")?;
        row.map(|row| {
            Ok(LastPrompt {
                prompt_tokens: read_tokens(&row, "prompt_tokens")?,
                through_sequence: read_tokens(&row, "through_sequence")?,
            })
        })
        .transpose()
    }

    /// Counts by author; `context` and tool definitions are left to the agent, which
    /// knows the request.
    pub async fn thread_tokens(&self, thread_id: &str) -> Result<ThreadTokens> {
        let mut tokens = ThreadTokens::default();
        let groups = sqlx::query(
            "SELECT role, role = 'system' OR sequence > COALESCE(
                    (SELECT through_sequence FROM compactions WHERE thread_id = ?
                     ORDER BY compaction_id DESC LIMIT 1), 0) AS in_context,
                SUM(content_tokens) AS content_tokens,
                SUM(tool_calls_tokens) AS tool_calls_tokens,
                SUM(reasoning_tokens) AS reasoning_tokens
             FROM messages WHERE thread_id = ? GROUP BY role, in_context",
        )
        .bind(thread_id)
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

        let summaries = sqlx::query(
            "SELECT COALESCE(SUM(summary_tokens), 0) AS all_tokens,
                COALESCE((SELECT summary_tokens FROM compactions WHERE thread_id = ?
                    ORDER BY compaction_id DESC LIMIT 1), 0) AS current_tokens
             FROM compactions WHERE thread_id = ?",
        )
        .bind(thread_id)
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await
        .context("count summary tokens")?;
        tokens.all.summary = read_tokens(&summaries, "all_tokens")?;
        tokens.current.summary = read_tokens(&summaries, "current_tokens")?;

        let spent = sqlx::query(
            "SELECT COUNT(*) AS model_calls,
                COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens,
                COALESCE(SUM(cached_prompt_tokens), 0) AS cached_prompt_tokens,
                COALESCE(SUM(completion_tokens), 0) AS completion_tokens,
                COALESCE(SUM(reasoning_tokens), 0) AS reasoning_tokens
             FROM model_usage WHERE thread_id = ?",
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

impl CallPurpose {
    fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Compact => "compact",
            Self::Title => "title",
        }
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
        state::{TurnAnswer, TurnOutcome},
        testing::temp_state,
    };

    /// A system prompt, a question, a tool step with reasoning and a final answer, as
    /// messages 1 to 5.
    async fn thread_with_a_tool_step(name: &str) -> (State, String) {
        let state = temp_state(name).await;
        state.create_thread("thread", "test").await.unwrap();
        state.add_system_message("thread", "1234").await.unwrap();
        state
            .add_user_message("thread", "uid", "12345678")
            .await
            .unwrap();
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        let thinking = TurnAnswer {
            content: String::new(),
            reasoning: Some("1234".into()),
        };
        let calls = vec![ToolCall {
            id: "call_1".into(),
            name: "ping".into(),
            arguments: json!({}),
        }];
        state
            .record_tool_calls(&turn.turn_id, thinking, calls)
            .await
            .unwrap();
        let result = state.record_tool_result(&turn.turn_id, "call_1", "pong".into());
        result.await.unwrap();
        let answer = TurnAnswer {
            content: "1234".into(),
            reasoning: None,
        };
        let outcome = TurnOutcome::Completed(answer);
        state.finish_turn(&turn.turn_id, outcome).await.unwrap();
        (state, turn.turn_id)
    }

    fn call(turn_id: &str, compaction_id: Option<i64>, through_sequence: u64) -> ModelCall {
        ModelCall {
            thread_id: "thread".into(),
            turn_id: Some(turn_id.into()),
            purpose: CallPurpose::Answer,
            compaction_id,
            model: "model".into(),
            through_sequence,
        }
    }

    #[tokio::test]
    async fn a_compaction_makes_older_prompt_counts_stale() {
        let (state, turn_id) = thread_with_a_tool_step("tokens_stale").await;
        let usage = Usage {
            prompt_tokens: 100,
            ..Usage::default()
        };
        state
            .record_usage(&call(&turn_id, None, 4), usage)
            .await
            .unwrap();
        let compaction = state.add_compaction("thread", 5, "summary").await.unwrap();

        let id = Some(compaction.compaction_id);
        assert_eq!(state.last_prompt("thread", id).await.unwrap(), None);
        state
            .record_usage(&call(&turn_id, id, 5), usage)
            .await
            .unwrap();
        assert!(state.last_prompt("thread", id).await.unwrap().is_some());
    }
}
