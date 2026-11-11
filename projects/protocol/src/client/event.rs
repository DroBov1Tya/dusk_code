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
}
