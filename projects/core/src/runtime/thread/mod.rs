//! A thread from start to end: the `launcher` that builds every thread, `start` for its
//! record and prompts, the `runner` that runs it and the `lock` that keeps other processes
//! off it; what the model reads of it, its `context`, `project` and `title`.

mod context;
mod launcher;
mod lock;
pub mod project;
mod runner;
pub mod start;
mod title;

pub use context::{ContextSource, Conversation, spec_tokens};
pub use launcher::Launcher;
pub use lock::ThreadLock;
pub use runner::{ThreadInput, ThreadOutput, ThreadRunner, ThreadSetup};
