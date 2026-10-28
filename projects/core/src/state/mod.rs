//! SQLite session records and in-memory events emitted after successful writes.

mod event_logs;
mod jobs;
mod messages;
// Named after its primary item, `State`, as the style guide asks.
#[allow(clippy::module_inception)]
mod state;
mod threads;
mod turns;

pub use jobs::{Job, JobInput, JobOutcome, JobStatus};
pub use messages::{Message, MessageAttachment, MessageInput, MessageRole};
pub use state::State;
pub use threads::{ApprovalMode, ThreadInput, ThreadMetadata};
pub use turns::{Turn, TurnStatus};

use state::timestamp_ms;
