//! What the agent does in a thread: its turns with their tool calls, the tokens each
//! model call used, the task board, the plan's team, the background jobs and the
//! Overview counted from them.

mod failure;
mod jobs;
mod overview;
mod resume;
mod tasks;
mod teams;
mod tokens;
mod turns;
mod waits;

pub use failure::{ProviderFailure, TurnFailure};
pub use jobs::{BackgroundJob, JobSummary};
pub use resume::{CHECK_NOTE, CUT_OFF, Settle, Unfinished};
pub use tasks::{
    BlockedOn, Claim, LEASE_MS, MAX_ATTEMPTS, MAX_DEPENDENCIES, MAX_FILES, MAX_OPEN_TASKS,
    MAX_REWORKS, MAX_TASKS_AN_ADD, NewTask, PassedBy, PlanError, PlanStatus, PlanTask, Task,
    TaskEdit, TaskFiles, Unclaimable, WaitsOn, overlaps,
};
pub use teams::{
    BudgetBy, BudgetPart, Spent, TeamAsk, TeamCeilings, TeamCheck, TeamCounts, TeamError, TeamRun,
};
pub use tokens::{CallPurpose, LastPrompt, ModelCall};
pub use turns::{Turn, TurnAnswer, TurnOutcome, TurnStatus};

use super::{Message, State, conversation::messages, timestamp_ms};
