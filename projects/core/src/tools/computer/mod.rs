//! Computer use: the model takes a screenshot when it needs to look, then clicks,
//! types, presses keys and scrolls by the coordinates of that picture. Offered only with
//! a screen, to a model that sees pictures, after one yes from the user a session.
//!
//! The desktop is macOS or Linux. On macOS, `window_session` pins captures and
//! semantic clicks to one app window. A macOS click presses the control and leaves the
//! user's pointer alone; only system mode, with a yes of its own, moves the pointer. A
//! screenshot can be of one window, and a cursor of dusk's own marks where the agent
//! last acted. On Linux every click moves the pointer, through the X11 or Wayland tools
//! the desktop has.
//!
//! The tools are the flat files here (`click`, `press_keys`, `screenshot`, `scroll`,
//! `type_text`, `window_session`) over `screen`, the screen as the model sees it, and
//! `offer`, how they are offered. `backend` is the desktop they drive; `overlay` is
//! dusk's own cursor on macOS.

#[macro_use]
mod offer;

mod backend;
mod click;
#[cfg(target_os = "macos")]
mod overlay;
mod press_keys;
mod screen;
mod screenshot;
mod scroll;
mod type_text;
#[cfg(target_os = "macos")]
mod window_session;

use anyhow::{Context, Result};
use backend::Desktop;

pub use click::Click;
#[cfg(target_os = "macos")]
pub use overlay::run_overlay_if_asked;
pub use press_keys::PressKeys;
pub use screen::Screen;
pub use screenshot::Screenshot;
pub use scroll::Scroll;
pub use type_text::TypeText;
#[cfg(target_os = "macos")]
pub use window_session::WindowSession;

/// A place on the display in points, from the top left corner of the main display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// A part of the display in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// An app's window on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    #[cfg(target_os = "macos")]
    pub pid: i32,
    pub id: u32,
    pub app: String,
    pub title: String,
    pub bounds: Rect,
}

/// Window identity and geometry when a screenshot was taken.
#[derive(Clone, Copy, Debug, PartialEq)]
struct WindowSnapshot {
    id: u32,
    #[cfg(target_os = "macos")]
    pid: i32,
    bounds: Rect,
}

impl From<&Window> for WindowSnapshot {
    fn from(window: &Window) -> Self {
        Self {
            id: window.id,
            #[cfg(target_os = "macos")]
            pid: window.pid,
            bounds: window.bounds,
        }
    }
}

/// One key with its modifiers, by a name every desktop knows: see [`screen::keys`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    pub key: String,
    pub cmd: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// Runs desktop work off the async runtime: every backend call blocks.
pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .context("the screen action stopped")?
}
