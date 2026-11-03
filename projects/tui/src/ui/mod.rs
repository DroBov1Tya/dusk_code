//! Drawing the app. The statistics rail fills the left edge on a colour of its own; the
//! thread and the message card take the rest, with air between them and no rule.

mod chat;
mod frame;
mod loader;
pub mod screensaver;
mod sidebar;
mod text;
mod theme;
mod windows;

pub use frame::{draw, tab_title};
