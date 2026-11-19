//! What changed in skills during turns, kept to undo it.

mod diff;
mod text;
mod watch;

pub use diff::{counts, diff};
pub use text::{MAX_REASON_CHARS, plain_line};
pub use watch::Watch;
