//! Task contracts and the in-process handler; no model provider is invoked.

mod execution;
mod handler;
mod task;

pub use execution::{Execution, ExecutionFailureCode, ExecutionStatus, RejectionCode};
pub use handler::Transport;
pub use task::Task;

mod database;
pub use database::{DatabasePassword, DatabaseTask, DatabaseTransport};
