//! How long the agent worked on a turn: from the boundary that opened it to the one that
//! closed it, both stamped on the clock, so a chat read from history shows it too.

/// When a turn began and, once it is over, when it ended, in Unix milliseconds.
pub type Span = (i64, Option<i64>);

pub use dusk_protocol::thread::duration_words as duration;
