//! Computer use on macOS: the model takes a screenshot when it needs to look, then
//! clicks, types, presses keys and scrolls by the coordinates of that picture. The
//! events move the system's own pointer and keyboard. Offered only with a screen, to a
//! model that sees pictures, after one yes from the user a session. A screenshot can be
//! of one window, and a cursor of dusk's own marks where the agent last acted.

mod cursor;
mod macos;
mod overlay;
mod screen;
mod tools;

pub use overlay::run_overlay_if_asked;
pub use screen::Screen;
pub use tools::{Click, PressKeys, Screenshot, Scroll, TypeText};
