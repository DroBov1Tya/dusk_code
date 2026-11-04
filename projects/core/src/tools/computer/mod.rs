//! Computer use: the model takes a screenshot when it needs to look, then clicks,
//! types, presses keys and scrolls by the coordinates of that picture. Offered only with
//! a screen, to a model that sees pictures, after one yes from the user a session.
//!
//! `screen` and the five tool modules are the same everywhere; the desktop they drive is `macos` or
//! `linux`. On macOS a click presses the control through Accessibility and leaves the
//! user's pointer alone; only system mode, with a yes of its own, moves the pointer. A
//! screenshot can be of one window, and a cursor of dusk's own marks where the agent
//! last acted. On Linux every click moves the pointer, through the X11 or Wayland tools
//! the desktop has.

#[macro_use]
mod shared;

#[cfg(target_os = "macos")]
mod accessibility;
mod click;
#[cfg(target_os = "macos")]
mod cursor;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod overlay;
mod press_keys;
mod screen;
mod screenshot;
mod scroll;
mod type_text;

#[cfg(target_os = "linux")]
use linux::Desktop;
#[cfg(target_os = "macos")]
use macos::Desktop;

pub use click::Click;
#[cfg(target_os = "macos")]
pub use overlay::run_overlay_if_asked;
pub use press_keys::PressKeys;
pub use screen::Screen;
pub use screenshot::Screenshot;
pub use scroll::Scroll;
pub use type_text::TypeText;

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
pub struct Window {
    pub id: u32,
    pub app: String,
    pub title: String,
    pub bounds: Rect,
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
