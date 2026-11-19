//! What every part of the core that reaches a server shares: reading an answer with a
//! hard cap, and which addresses it may connect to on someone else's word.

pub mod body;
pub mod public;

pub use body::{BodyError, read_limited, read_limited_reporting};
