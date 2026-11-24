//! Goals: what `/goal` sets and how a chat works toward it. `command` reads the command
//! and writes the messages that start and remind each turn; `evaluate` asks a separate
//! model call whether the goal is met; `rounds` judges each turn and starts the next, and
//! `stall` pauses a goal whose rounds stop changing anything. The records are in
//! `state::thread::goal`.

pub mod command;
mod evaluate;
mod rounds;
mod stall;

/// Why a goal the user stopped is paused.
pub const STOPPED: &str = "stopped by the user";
