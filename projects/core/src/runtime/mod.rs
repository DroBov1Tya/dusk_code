//! The runtime, in three parts: `agent` runs one thread's agent (its steps, model calls,
//! compaction and the messages waiting for it), `chat` builds what the model reads, and
//! `system` starts and keeps the threads that clients and agents open.

pub mod agent;
pub mod chat;
pub mod system;

pub use agent::AUTO_COMPACT_PERCENT;
pub use system::{Runtime, ThreadEvent, ThreadHandle, ThreadInput, ThreadOutput, model_choice};
