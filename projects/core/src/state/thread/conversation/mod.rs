//! What is said in a thread: its messages, those waiting for the next turn or kept
//! unsent, what compactions left of the older ones, the points it can be undone to and
//! the sources attached to it.

mod compactions;
mod local_unsent;
pub mod messages;
mod pending;
mod sources;
mod undo;

pub use compactions::Compaction;
pub use local_unsent::LocalUnsent;
pub use messages::{Message, MessageRole, estimate_tokens};
pub use pending::PendingMessage;
pub use sources::Source;
pub use undo::UndoPoint;

use super::{State, timestamp_ms};
