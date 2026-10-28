//! The agent: live sessions that run model turns over a thread.

mod context;
mod generation;
mod session_runner;

pub use session_runner::{SessionEvent, SessionInput, SessionOutput, SessionRunner};
