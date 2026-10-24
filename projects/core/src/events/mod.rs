//! Concrete event payloads, aggregate event contract, and Tokio broadcast types.

mod event;
mod execution_failed;
mod job_created;
mod job_updated;
mod lifecycle_changed;
mod message_created;
mod task_accepted;
mod task_rejected;
mod thread_created;
mod thread_updated;

pub use event::Event;
pub use execution_failed::ExecutionFailed;
pub use job_created::JobCreated;
pub use job_updated::JobUpdated;
pub use lifecycle_changed::LifecycleChanged;
pub use message_created::MessageCreated;
pub use task_accepted::TaskAccepted;
pub use task_rejected::TaskRejected;
pub use thread_created::ThreadCreated;
pub use thread_updated::ThreadUpdated;

use tokio::sync::broadcast;

/// Compatibility name for existing event consumers.
pub type CoreEvent = Event;

pub type EventSender = broadcast::Sender<CoreEvent>;
pub type EventReceiver = broadcast::Receiver<CoreEvent>;

// Consumers must handle Tokio broadcast lag; durable delivery is not provided.

mod database_created;
pub use database_created::DatabaseCreated;
