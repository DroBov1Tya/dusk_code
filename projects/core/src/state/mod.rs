//! SQLite records, in three parts: `db` opens the database and keeps its schema current,
//! old databases included; `thread` holds what a thread is made of, from its messages and
//! turns to its token usage and task board; `agents` holds the agent identities.

mod agents;
pub mod artifacts;
mod db;
mod thread;

pub use agents::Identity;
pub use db::State;
pub use thread::{
    AgentPhase, BackgroundJob, CallPurpose, Compaction, Goal, GoalCheck, GoalCriterion,
    GoalEvidence, GoalStatus, JobSummary, LastPrompt, LocalUnsent, Message, MessageRole, ModelCall,
    PendingMessage, Source, Task, TaskItem, Thread, ThreadTokens, TokenCounts, Turn, TurnAnswer,
    TurnOutcome, TurnStatus, UndoPoint, estimate_tokens,
};

pub use db::timestamp_ms;
