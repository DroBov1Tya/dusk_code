//! What a chat message carries besides plain text: commands a client sends, the
//! standing goal they and the model set, text the user pasted, and tags some models
//! write inline. Providers only move bytes; the meaning of a message is worked out here.
//! `plain` is the other direction: text of a tool or a model made safe to quote.

pub mod commands;
pub mod path_completion;
pub mod plain;
pub mod sources;
