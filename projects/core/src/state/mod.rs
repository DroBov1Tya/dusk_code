//! SQLite records, in three parts: `db` opens the database and keeps its schema current,
//! old databases included; `chat` holds what a thread is made of, from its messages and
//! turns to its token usage and task board; `agents` holds the agent identities.

mod agents;
pub mod artifacts;
mod chat;
mod db;

pub use agents::Identity;
pub use chat::{
    BackgroundJob, CallPurpose, Compaction, Goal, GoalStatus, JobSummary, LastPrompt, LocalUnsent,
    Message, MessageRole, ModelCall, PendingMessage, Task, TaskItem, Thread, ThreadTokens,
    TokenCounts, Turn, TurnAnswer, TurnOutcome, TurnStatus, UndoPoint, estimate_tokens,
};
pub use db::State;

use db::timestamp_ms;
