//! Task contracts and the in-process handler; no model provider is invoked.

mod database_transport;
mod execution;
mod task;
// Named after its primary item, `Transport`, as the style guide asks.
#[allow(clippy::module_inception)]
mod transport;

pub use database_transport::{DatabasePassword, DatabaseTask, DatabaseTransport};
pub use execution::{Execution, ExecutionFailureCode, ExecutionStatus, RejectionCode};
pub use task::Task;
pub use transport::Transport;
