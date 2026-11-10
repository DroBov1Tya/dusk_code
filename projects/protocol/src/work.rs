//! A thread's turns, its task board and its token counts.

use serde::{Deserialize, Serialize};

/// Compacting starts once the context passes this share of the input limit. The rest is
/// room for the summary request and its answer, so 256k compacts at about 205k.
pub const AUTO_COMPACT_PERCENT: u64 = 80;

use crate::model::Usage;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub turn_id: String,
    pub thread_id: String,
    /// Sequence of the newest user message this turn answers.
    pub last_sequence: u64,
    pub response_message_id: Option<String>,
    pub status: TurnStatus,
    /// Unix timestamps in milliseconds.
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnStatus {
    Running,
    Completed,
    /// Stopped by the user or a restart; a partial answer is kept when there was one.
    Interrupted,
    Failed,
}

impl TurnStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub description: String,
    pub done: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// In order.
    pub items: Vec<TaskItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskItem {
    pub item_id: String,
    pub description: String,
    pub done: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// `None` until work on it begins.
    pub started_at: Option<i64>,
    /// Set exactly when it is done.
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadTokens {
    /// The best known size of the current context: what the provider counted for the
    /// last call plus estimates of what came after it. Before any call, `current.total()`.
    pub context: u64,
    /// What the model receives now: the system messages, the newest summary and the
    /// messages after it.
    pub current: TokenCounts,
    /// Every message and summary the thread ever had.
    pub all: TokenCounts,
    /// Sums of what the provider reported over every model call of the thread and of the
    /// subagents it asked.
    pub spent: Usage,
    pub model_calls: u64,
    /// The summed cost of the calls whose cost is known.
    #[serde(default)]
    pub cost_micro_usd: u64,
    /// Calls with no known cost: the provider did not report it and no price was set.
    #[serde(default)]
    pub unpriced_calls: u64,
}

impl ThreadTokens {
    /// What the thread cost, as `$0.42`; `+` when some calls have no known cost. None
    /// when no call has one, so free and local models show nothing.
    pub fn cost(&self) -> Option<String> {
        if self.unpriced_calls >= self.model_calls {
            return None;
        }
        let more = if self.unpriced_calls > 0 { "+" } else { "" };
        Some(format!(
            "{}{more}",
            crate::model::Cost::dollars(self.cost_micro_usd)
        ))
    }
}

/// Estimated tokens by who wrote them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCounts {
    /// The mode's prompt and other system instructions.
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

    pub fn add(&mut self, role: &str, content: u64, tool_calls: u64, reasoning: u64) {
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

/// A job as a client lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInfo {
    pub job_id: String,
    pub command: String,
    /// In words: running, completed, failed, cancelled, timed_out, interrupted, or running
    /// in, or unknown because of, another runtime.
    pub status: String,
    pub running: bool,
    /// This runtime owns its process, so it can be stopped from here.
    pub stoppable: bool,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}
