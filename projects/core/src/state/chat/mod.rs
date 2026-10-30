//! What a thread is made of: its record, messages, turns, the messages waiting for it,
//! compactions, token usage and the agent's task board.

mod compactions;
mod local_unsent;
mod messages;
mod pending;
mod tasks;
mod threads;
mod tokens;
mod turns;

pub use compactions::Compaction;
pub use local_unsent::LocalUnsent;
pub use messages::{Message, MessageRole, estimate_tokens};
pub use pending::PendingMessage;
pub use tasks::{Task, TaskItem};
pub use threads::Thread;
pub use tokens::{CallPurpose, LastPrompt, ModelCall, ThreadTokens, TokenCounts};
pub use turns::{Turn, TurnAnswer, TurnOutcome, TurnStatus};

use super::{State, timestamp_ms};
