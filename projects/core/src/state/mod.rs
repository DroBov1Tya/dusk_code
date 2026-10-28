//! SQLite records of threads, messages, turns, compactions and token usage.

mod compactions;
mod messages;
// Named after its primary item, `State`, as the style guide asks.
#[allow(clippy::module_inception)]
mod state;
mod threads;
mod tokens;
mod turns;

pub use compactions::{Compaction, CompactionReason};
pub(crate) use messages::estimate_tokens;
pub use messages::{Message, MessageRole};
pub use state::State;
pub use threads::Thread;
pub use tokens::{CallPurpose, LastPrompt, ModelCall, ThreadTokens, TokenCounts};
pub use turns::{StepResult, Turn, TurnAnswer, TurnOutcome, TurnStatus, TurnStep};

use state::timestamp_ms;
