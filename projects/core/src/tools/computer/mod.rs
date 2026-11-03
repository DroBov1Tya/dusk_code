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

pub use screen::Screen;
pub use tools::{Click, PressKeys, Screenshot, Scroll, TypeText};

/// In the cursor's helper process, draws the cursor until its parent goes and returns
/// true; otherwise returns false at once. A client calls it first thing in `main`, on
/// the main thread, which AppKit needs.
pub fn run_overlay_if_asked() -> bool {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == cursor::OVERLAY_ARG)
    {
        overlay::run();
        return true;
    }
    false
}
