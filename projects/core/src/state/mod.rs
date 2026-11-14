//! SQLite records, in parts: `db` opens the database and keeps its schema current,
//! old databases included; `thread` holds what a thread is made of, from its messages and
//! turns to its token usage and task board; `agents` holds the agent identities and what
//! each agent's thread keeps of its own; `project` holds named folders, their variables
//! and schedules; `usage` holds the durable counts of skills and
//! MCP tools the model used.

mod agents;
pub mod artifacts;
mod db;
mod project;
mod thread;
mod usage;

pub use agents::{AgentPhase, Identity};
pub use db::State;
pub use thread::{
    BackgroundJob, CallPurpose, Compaction, Goal, GoalCheck, GoalCriterion, GoalEvidence,
    GoalStatus, JobSummary, LastPrompt, LocalUnsent, Message, MessageRole, ModelCall,
    PendingMessage, Source, Task, Thread, Turn, TurnAnswer, TurnOutcome, TurnStatus, UndoPoint,
    estimate_tokens,
};
pub use usage::{ItemUsage, KIND_MCP_TOOL, KIND_SKILL};

pub use db::timestamp_ms;
