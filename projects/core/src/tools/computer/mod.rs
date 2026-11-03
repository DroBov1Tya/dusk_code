//! Computer use on macOS: the model takes a screenshot when it needs to look, then
//! clicks, types, presses keys and scrolls by the coordinates of that picture. The
//! events move the system's own pointer and keyboard. Offered only with a screen, to a
//! model that sees pictures, after one yes from the user a session.

mod macos;
mod screen;
mod tools;

pub use screen::Screen;
pub use tools::{Click, PressKeys, Screenshot, Scroll, TypeText};
