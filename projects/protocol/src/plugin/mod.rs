//! Plugins that bring their own interface: what a plugin may do, where its parts sit in
//! the window, the views it draws with the host's widgets and the messages it trades
//! with the core. A plugin is a separate process; nothing here lets it reach the core's
//! secrets.
//!
//! - `permission`: what a plugin asks to be allowed, and nothing more is;
//! - `place`: where a plugin's part shows and what a click on it opens;
//! - `view`: the widget tree a part is drawn from, and the events it sends back;
//! - `wire`: the messages between the core and the plugin's process.

mod permission;
mod place;
mod view;
mod wire;

pub use permission::{KeyUse, Permissions};
pub use place::{Opens, Part, Place};
pub use view::{ListItem, MAX_NODES, MAX_TEXT, Size, Tone, UiEvent, Widget, check_view};
pub use wire::{FromPlugin, MAX_FRAME, MAX_LINE, ToPlugin};
