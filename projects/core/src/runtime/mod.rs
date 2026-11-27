//! The runtime: `Runtime` in `start`, what clients start, with its calls in `api`;
//! `thread`, a thread from its launcher to its runner and lock and what the model reads of
//! it; `agent`, the agent of one thread (its steps, model calls, compaction and the
//! messages between agents); `team`, the core's run of a chat's plan by members; the model
//! each chat talks to in `model_choice`, and the strong, default and weak models a subagent may
//! be asked on in `tiers`; goals and the evaluator that judges them; the arbiter; temporary
//! chats; and the background jobs.

pub mod agent;
mod api;
pub mod arbiter;
pub mod goal;
pub mod jobs;
pub mod model_choice;
mod start;
pub mod team;
mod temporary;
pub mod thread;
pub mod tiers;

pub use dusk_protocol::thread::ThreadEvent;
pub use start::{Runtime, ThreadHandle};
pub use temporary::folders as temporary_folders;
pub use thread::{Launcher, ThreadInput, ThreadLock, ThreadOutput, ThreadRunner, ThreadSetup};
