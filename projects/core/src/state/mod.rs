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
mod skill_changes;
mod thread;
mod usage;

pub use agents::{AgentPhase, Identity};
pub use db::State;
pub use project::{Episode, Recall};
pub use project::{EpisodeKind, NewEpisode};
pub use skill_changes::{NewSkillChange, StoredSkillChange};
pub(crate) use thread::below;
pub use thread::{
    BackgroundJob, CHECK_NOTE, CUT_OFF, CallEnd, CallPurpose, ChangeQuery, Compaction, Delivery,
    Ended, Goal, GoalCheck, GoalCriterion, GoalEvidence, GoalStatus, JobSummary, LastPrompt,
    LocalUnsent, Message, MessageRole, ModelCall, NewStub, PendingMessage, ProviderFailure, Settle,
    ShapedResult, Source, Stub, Task, Thread, Turn, TurnAnswer, TurnFailure, TurnOutcome,
    TurnStatus, UndoPoint, Unfinished, calls_of, estimate_tokens, saved_tokens,
};
pub use thread::{MEMBER_SOURCE, short_task_id, slim_pull};
// The plan's store. Its callers are the team's runtime (T1) and the plan form of the
// `tasks` tool (L1); until they land only its tests use these.
#[allow(unused_imports)]
pub use thread::{
    BlockedOn, Claim, LEASE_MS, MAX_ATTEMPTS, MAX_DEPENDENCIES, MAX_FILES, MAX_NOTE,
    MAX_OPEN_TASKS, MAX_REWORKS, MAX_TASKS_AN_ADD, NewTask, PassedBy, PlanError, PlanStatus,
    PlanTask, TaskEdit, TaskFiles, Unclaimable, WaitsOn, overlaps,
};
// The team's store. Its callers are the team's runtime (T1), serving (P1) and recovery
// (R1); until they land only its tests use these.
#[allow(unused_imports)]
pub use thread::{
    BudgetBy, BudgetPart, Spent, TeamAsk, TeamCeilings, TeamCheck, TeamCounts, TeamError, TeamRun,
};
pub use usage::{ItemUsage, KIND_MCP_TOOL, KIND_SKILL};

pub use db::timestamp_ms;
