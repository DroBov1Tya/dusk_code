//! The client's session with the core: `run` is the loop, `actions` what each action
//! does, `answers` what the core answers to commands, `thread` the thread on screen,
//! `tabs` the threads of the other tabs.

mod actions;
mod answers;
mod run;
mod schedules;
mod tabs;
mod thread;

pub use run::{THREAD_SOURCE, run};
