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
// Used by tasks E4 and E5 (`recall`, clients); until they land only tests do.
#[allow(unused_imports)]
pub use project::{Episode, Recall};
pub use project::{EpisodeKind, NewEpisode};
pub use thread::{
    BackgroundJob, CHECK_NOTE, CUT_OFF, CallEnd, CallPurpose, ChangeQuery, Compaction, Ended, Goal,
    GoalCheck, GoalCriterion, GoalEvidence, GoalStatus, JobSummary, LastPrompt, LocalUnsent,
    Message, MessageRole, ModelCall, NewStub, PendingMessage, ProviderFailure, Settle,
    ShapedResult, Source, Stub, Task, Thread, Turn, TurnAnswer, TurnFailure, TurnOutcome,
    TurnStatus, UndoPoint, Unfinished, calls_of, estimate_tokens, saved_tokens,
};
pub use usage::{ItemUsage, KIND_MCP_TOOL, KIND_SKILL};

pub use db::timestamp_ms;
