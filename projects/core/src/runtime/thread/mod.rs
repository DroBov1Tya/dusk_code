//! A thread from start to end: the `launcher` that builds every thread, `start` for its
//! record and prompts, the `runner` that runs it and the `lock` that keeps other processes
//! off it; what the model reads of it, its `context`, `project` and `title`; the tests of
//! the request's start (`prefix`) and of its stubbed results (`stub_view`).

mod chat_model;
mod context;
mod launcher;
mod lock;
#[cfg(test)]
mod prefix;
pub mod project;
mod runner;
pub mod start;
#[cfg(test)]
mod stub_view;
mod title;

pub use chat_model::ChatModel;
pub use context::{ContextSource, Conversation, spec_tokens};
pub use launcher::Launcher;
pub use lock::ThreadLock;
pub use runner::{ThreadInput, ThreadOutput, ThreadRunner, ThreadSetup};
