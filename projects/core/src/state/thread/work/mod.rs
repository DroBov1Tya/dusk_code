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
    BlockedOn, Claim, Duty, LEASE_MS, MAX_ATTEMPTS, MAX_DEPENDENCIES, MAX_FILES, MAX_GOAL,
    MAX_NOTE, MAX_OPEN_TASKS, MAX_RESULT, MAX_REWORKS, MAX_TASKS_AN_ADD, NewTask, PassedBy,
    PlanError, PlanStatus, PlanTask, Task, TaskEdit, TaskFiles, Unclaimable, WaitsOn, current_wave,
    overlaps, short_task_id, waves,
};
pub use teams::{
    TeamAsk, TeamCheck, TeamCounts, TeamError, TeamRun, WindWhy, Winding, member_summary,
};
pub use tokens::{CallPurpose, LastPrompt, ModelCall};
pub use turns::{Turn, TurnAnswer, TurnOutcome, TurnStatus};

use super::{Message, State, conversation::messages, timestamp_ms};
