//! dusk's own cursor over the screen on macOS, so the user sees where the agent acts.
//! `cursor` is the core's side, which starts the helper and tells it each point;
//! `helper` is the helper process, which draws the arrow in a click-through window.

mod cursor;
mod helper;

pub use cursor::Cursor;
pub use helper::run_overlay_if_asked;
