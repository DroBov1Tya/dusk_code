//! The client's session with the core: `run` is the loop, `actions` what each action
//! does, `answers` what the core answers to commands, `thread` the thread on screen.

mod actions;
mod answers;
mod run;
mod thread;

pub use run::{THREAD_SOURCE, run};
