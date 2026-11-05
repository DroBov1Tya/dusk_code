//! What a chat message carries besides plain text: commands a client sends, the
//! standing goal they and the model set, text the user pasted, and tags some models
//! write inline. Providers only move bytes; the meaning of a message is worked out here.

pub mod commands;
pub use dusk_protocol::pasted;
pub mod path_completion;
pub mod sources;
