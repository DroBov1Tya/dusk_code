//! The runtime: what clients start, and the agent that runs each thread.

pub mod agent;
mod compaction;
mod context;
mod generation;
pub mod inbox;
pub mod launcher;
mod mailboxes;
pub mod model_choice;
mod thread_lock;
// Named after its primary item, `Runtime`, as the style guide asks.
#[allow(clippy::module_inception)]
mod runtime;
mod thread_runner;
mod title;

pub use compaction::AUTO_COMPACT_PERCENT;
pub use runtime::{Runtime, ThreadHandle};
pub use thread_runner::{ThreadEvent, ThreadInput, ThreadOutput};
