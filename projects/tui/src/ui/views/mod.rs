//! Screen regions and overlay windows.

pub mod agent_window;
pub mod header;
pub mod input;
pub mod panels;
pub mod settings;
pub mod sidebar;
pub mod transcript;

use super::{
    loader,
    text::{markdown, wrap},
    theme,
};
