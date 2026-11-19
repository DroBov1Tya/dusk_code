//! What changed in skills during turns, kept to undo it.

mod diff;
mod text;

pub use diff::diff;
// Caller is the skill-evolve tool (Task 5).
#[allow(unused_imports)]
pub use text::{MAX_REASON_CHARS, plain_line};
