//! Message composition, pasted text chips and the paste editor.

mod editor;
mod input;
pub mod paste;
mod paths;
mod suggest;

use super::{App, Panel, compact_number};

pub use editor::{EditorButton, EditorHit, EditorTarget, PasteEditor};
pub use input::Input;
pub use paths::PathPopup;
