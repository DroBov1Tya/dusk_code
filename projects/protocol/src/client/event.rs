//! What the core pushes to a client on its own.

use serde::{Deserialize, Serialize};

use crate::thread::ThreadEvent;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Thread {
        thread_id: String,
        event: Box<ThreadEvent>,
    },
    /// The thread's task ended; sending to it opens it again.
    Ended { thread_id: String },
    /// What a terminal printed, as raw bytes in the order they came; sent only to the
    /// client that opened it.
    Terminal { id: String, data: Vec<u8> },
    /// The terminal's shell ended, by itself or because the terminal was closed. `code`
    /// is its exit code, or none when a signal ended it.
    TerminalClosed { id: String, code: Option<i32> },
}
