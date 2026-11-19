//! What changed in skills during turns, kept to undo it.

mod text;

// Callers are the skill-evolve tool and the undo request (Tasks 4 and 5).
#[allow(unused_imports)]
pub use text::{MAX_REASON_CHARS, plain_line};
