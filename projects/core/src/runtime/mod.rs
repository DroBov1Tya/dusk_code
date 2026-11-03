//! The runtime: `Runtime`, what clients start; the `launcher` that builds every thread
//! and starts it; each thread's `thread_runner` and its lock; the model all threads talk
//! to; `agent`, the agent of one thread (its steps, model calls, compaction and the
//! messages waiting for it); `thread`, what the model reads; goals and their
//! verification; the arbiter; and the background jobs.

pub mod agent;
pub mod arbiter;
mod events;
mod goal_verification;
pub mod jobs;
mod launcher;
pub mod model_choice;
// Named after its primary item, `Runtime`, as the style guide asks.
#[allow(clippy::module_inception)]
mod runtime;
pub mod thread;
mod thread_lock;
mod thread_runner;

pub use agent::AUTO_COMPACT_PERCENT;
pub use events::ThreadEvent;
pub use launcher::Launcher;
pub use runtime::{Runtime, SystemReport, ThreadHandle};
pub use thread_lock::ThreadLock;
pub use thread_runner::{ThreadInput, ThreadOutput, ThreadRunner, ThreadSetup};
