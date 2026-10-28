//! SQLite records of threads, messages and turns, with live events after each write.

mod messages;
// Named after its primary item, `State`, as the style guide asks.
#[allow(clippy::module_inception)]
mod state;
mod threads;
mod turns;

pub use messages::{Message, MessageRole, estimate_tokens};
pub use state::{DatabasePassword, State};
pub use threads::Thread;
pub use turns::{StepResult, Turn, TurnAnswer, TurnOutcome, TurnStatus, TurnStep};

use state::timestamp_ms;
