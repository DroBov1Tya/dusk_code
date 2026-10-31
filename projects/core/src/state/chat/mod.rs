//! What a thread is made of: its record, messages, turns, the messages waiting for it,
//! compactions, token usage, the agent's task board and the standing goal.

mod compactions;
mod goals;
mod jobs;
mod local_unsent;
mod messages;
mod pending;
mod tasks;
mod threads;
mod tokens;
mod turns;
mod undo;

pub use compactions::Compaction;
pub use goals::{Goal, GoalStatus};
pub use jobs::{BackgroundJob, JobSummary};
pub use local_unsent::LocalUnsent;
pub use messages::{Message, MessageRole, estimate_tokens};
pub use pending::PendingMessage;
pub use tasks::{Task, TaskItem};
pub use threads::Thread;
pub use tokens::{CallPurpose, LastPrompt, ModelCall, ThreadTokens, TokenCounts};
pub use turns::{Turn, TurnAnswer, TurnOutcome, TurnStatus};
pub use undo::UndoPoint;

use super::{State, timestamp_ms};
