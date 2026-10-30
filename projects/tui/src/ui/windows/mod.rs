//! Windows over the conversation: lists, settings, a subagent's chat.

pub mod agent_window;
pub mod panels;
pub mod settings;

use super::{
    chat::{input, transcript},
    loader, sidebar,
    text::{markdown, wrap},
    theme,
};
