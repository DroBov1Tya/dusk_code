//! The messages between the core and a plugin's interface process: one JSON object per
//! line on its stdin and stdout. Frames are base64 in the line, as a line is all there
//! is.
//!
//! Nothing here, or in any message a plugin is sent, carries a secret of the user's: no
//! provider key, sign-in token or vault entry ever reaches a plugin, and no type a plugin
//! sees may hold a `Secret`. A plugin's own keys stay in the core's broker.

use serde::{Deserialize, Serialize};

use super::view::{UiEvent, Widget};

/// The longest line the core reads from a plugin.
pub const MAX_LINE: usize = 4 * 1024 * 1024;
/// The largest frame a plugin may send, decoded.
pub const MAX_FRAME: usize = 3 * 1024 * 1024;

/// From the core to the plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToPlugin {
    /// A part was opened; the plugin answers with its view.
    Open { part: String },
    /// A part was closed; the plugin may stop its work for it.
    Close { part: String },
    /// A widget of an open part was used.
    Event {
        part: String,
        widget: String,
        event: UiEvent,
    },
}

/// From the plugin to the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromPlugin {
    /// The whole view of a part, which replaces the one before.
    View { part: String, root: Widget },
    /// A new picture for a `screen` widget, as JPEG in base64.
    Frame {
        part: String,
        widget: String,
        jpeg: String,
    },
    /// A short note the window shows for a moment.
    Notice { text: String },
}
