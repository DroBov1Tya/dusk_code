//! The conversation: the thread name above, the transcript, and the message card.

pub mod header;
pub mod input;
pub mod transcript;

use super::{
    loader,
    text::{markdown, wrap},
    theme,
};
