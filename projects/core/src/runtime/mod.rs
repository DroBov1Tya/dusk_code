//! The runtime: `Runtime` in `start`, what clients start, with its calls in `api`;
//! `thread`, a thread from its launcher to its runner and lock and what the model reads of
//! it; `agent`, the agent of one thread (its steps, model calls, compaction and the
//! messages between agents); the model all threads talk to in `model_choice`; goals and
//! their verification; the arbiter; temporary chats; and the background jobs.

pub mod agent;
mod api;
pub mod arbiter;
pub mod goal;
pub mod jobs;
pub mod model_choice;
mod start;
mod temporary;
pub mod thread;

pub use dusk_protocol::thread::ThreadEvent;
pub use start::{Runtime, ThreadHandle};
pub use thread::{Launcher, ThreadInput, ThreadLock, ThreadOutput, ThreadRunner, ThreadSetup};
