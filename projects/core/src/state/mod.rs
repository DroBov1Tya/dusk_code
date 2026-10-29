//! SQLite records of threads, messages, turns, compactions and token usage.

mod compactions;
mod legacy;
mod local_unsent;
mod messages;
mod pending;
// Named after its primary item, `State`, as the style guide asks.
#[allow(clippy::module_inception)]
mod state;
mod tasks;
mod threads;
mod tokens;
mod turns;

pub use compactions::Compaction;
pub use local_unsent::LocalUnsent;
pub use messages::estimate_tokens;
pub use messages::{Message, MessageRole};
pub use pending::PendingMessage;
pub use state::State;
pub use tasks::{Task, TaskItem};
pub use threads::Thread;
pub use tokens::{CallPurpose, LastPrompt, ModelCall, ThreadTokens, TokenCounts};
pub use turns::{Turn, TurnAnswer, TurnOutcome, TurnStatus};

use state::timestamp_ms;
