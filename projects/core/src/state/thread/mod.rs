//! What a thread is made of: its record, then in three parts the conversation (its
//! messages, the messages waiting for it, compactions, undo points and sources), the
//! work done in it (turns, token usage, the task board and background jobs) and its
//! standing goal. `files` keeps the files its commands touched, `changes` the ledger of what
//! they changed in them.

mod approvals;
mod changes;
mod conversation;
mod files;
mod goal;
mod mode;
mod pulls;
mod threads;
mod unread;
mod work;

pub use changes::ChangeQuery;
pub use conversation::{
    CallEnd, Compaction, Delivery, Ended, LocalUnsent, Message, MessageRole, NewStub,
    PendingMessage, ShapedResult, Source, Stub, UndoPoint, calls_of, estimate_tokens, saved_tokens,
};
pub use goal::{Goal, GoalCheck, GoalCriterion, GoalEvidence, GoalStatus};
pub use pulls::slim_pull;
pub(crate) use threads::below;
pub use threads::{MEMBER_SOURCE, Thread};
pub use work::{
    BackgroundJob, CHECK_NOTE, CUT_OFF, CallPurpose, JobSummary, LastPrompt, ModelCall,
    ProviderFailure, Settle, Task, Turn, TurnAnswer, TurnFailure, TurnOutcome, TurnStatus,
    Unfinished,
};
pub use work::{
    BlockedOn, Claim, Duty, LEASE_MS, MAX_ATTEMPTS, MAX_DEPENDENCIES, MAX_FILES, MAX_GOAL,
    MAX_NOTE, MAX_OPEN_TASKS, MAX_RESULT, MAX_REWORKS, MAX_TASKS_AN_ADD, NewTask, PassedBy,
    PlanError, PlanStatus, PlanTask, TaskEdit, TaskFiles, Unclaimable, WaitsOn, current_wave,
    overlaps, short_task_id, waves,
};
pub use work::{TeamAsk, TeamCheck, TeamCounts, TeamError, TeamRun, WindWhy, Winding};

use super::{State, timestamp_ms};
