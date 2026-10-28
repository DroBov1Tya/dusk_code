//! Provider-neutral chat input and streamed output.

use std::future::Future;

use anyhow::Result;

/// Author of a chat message as the model understands it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

/// Exactly what the model sees: a role and text, without core metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

/// One increment of a streamed answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChatDelta {
    /// Model thinking: shown to the user and never sent back as context.
    Reasoning(String),
    /// Part of the answer.
    Text(String),
}

/// A streamed model answer.
pub trait ChatStream: Send {
    /// Returns `None` once the model has finished; an error ends the stream.
    fn next_delta(&mut self) -> impl Future<Output = Result<Option<ChatDelta>>> + Send;
}
