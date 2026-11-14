//! Windows over the conversation: lists, settings, a subagent's chat.

pub mod agent_window;
pub mod panels;
pub mod settings;

use super::{
    loader,
    text::{markdown, wrap},
    theme,
    thread::{input, transcript},
};
