//! The runtime: what clients start, and the agent that runs each thread.

mod agent;
mod compaction;
mod context;
mod generation;
mod inbox;
// Named after its primary item, `Runtime`, as the style guide asks.
#[allow(clippy::module_inception)]
mod runtime;
mod thread_runner;
mod title;

pub use runtime::{Runtime, ThreadHandle};
pub use thread_runner::{ThreadEvent, ThreadInput, ThreadOutput};
