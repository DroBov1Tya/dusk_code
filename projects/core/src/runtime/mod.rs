//! The runtime, in three parts: `agent` runs one thread's agent (its steps, model calls,
//! compaction and the messages waiting for it), `thread` builds what the model reads, and
//! `system` starts and keeps the threads that clients and agents open.

pub mod agent;
mod events;
pub mod system;
pub mod thread;

pub use agent::AUTO_COMPACT_PERCENT;
pub use events::ThreadEvent;
pub use system::{Runtime, SystemReport, ThreadHandle, ThreadInput, ThreadOutput, model_choice};
