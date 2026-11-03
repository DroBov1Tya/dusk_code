//! Anthropic Messages API. Thinking blocks are not sent back: the history keeps no
//! signatures, and a history without thinking blocks is one the API accepts on every
//! model.

mod client;
mod request;
mod stream;

pub use client::Anthropic;
pub use stream::MessageStream;
