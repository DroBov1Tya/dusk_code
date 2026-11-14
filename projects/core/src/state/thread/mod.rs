//! What a thread is made of: its record, then in three parts the conversation (its
//! messages, the messages waiting for it, compactions, undo points and sources), the
//! work done in it (turns, token usage, the task board and background jobs) and its
//! standing goal. `files` keeps the files its commands touched.

mod approvals;
mod conversation;
mod files;
mod goal;
mod threads;
mod work;

pub use conversation::{
    Compaction, LocalUnsent, Message, MessageRole, PendingMessage, Source, UndoPoint,
    estimate_tokens,
};
pub use goal::{Goal, GoalCheck, GoalCriterion, GoalEvidence, GoalStatus};
pub use threads::Thread;
pub use work::{
    BackgroundJob, CallPurpose, JobSummary, LastPrompt, ModelCall, Task, Turn, TurnAnswer,
    TurnOutcome, TurnStatus,
};

use super::{State, timestamp_ms};
