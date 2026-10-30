//! What a chat message carries besides plain text: commands a client sends, the
//! standing goal they and the model set, text the user pasted, and tags some models
//! write inline. Providers only move bytes; the meaning of a message is worked out here.

pub mod commands;
pub mod complete;
pub mod goal;
pub mod inline_tags;
pub mod pasted;
