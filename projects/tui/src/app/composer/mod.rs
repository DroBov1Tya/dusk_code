//! Message composition, pasted text chips and the paste editor.

mod editor;
mod input;
pub mod paste;

use super::{App, Panel, compact_number};

pub use editor::{EditorButton, EditorHit, PasteEditor};
pub use input::Input;
