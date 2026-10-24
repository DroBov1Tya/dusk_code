//! Async harness core contracts, transport, and persistent request metadata.

pub mod api;
pub mod events;
pub mod extensions;
pub mod lifecycle;
pub mod persistence;
pub mod policy;
pub mod state;
pub mod supervision;
pub mod transport;

pub use api::{Execution, ExecutionStatus, RejectionCode, Task};
pub use events::Event;
