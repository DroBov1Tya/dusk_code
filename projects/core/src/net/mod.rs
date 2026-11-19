//! What every part of the core that reaches a server shares: reading an answer with a
//! hard cap.

pub mod body;

pub use body::{BodyError, read_limited, read_limited_reporting};
