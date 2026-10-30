//! SQLite records, in three parts: `db` opens the database and keeps its schema current,
//! old databases included; `chat` holds what a thread is made of, from its messages and
//! turns to its token usage and task board; `agents` holds the agent identities.

mod agents;
mod chat;
mod db;

pub use agents::Identity;
pub use chat::{
    CallPurpose, Compaction, LastPrompt, LocalUnsent, Message, MessageRole, ModelCall,
    PendingMessage, Task, TaskItem, Thread, ThreadTokens, TokenCounts, Turn, TurnAnswer,
    TurnOutcome, TurnStatus, estimate_tokens,
};
pub use db::State;

use db::timestamp_ms;
