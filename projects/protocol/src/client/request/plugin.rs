//! The parts plugins add to the window: opening one, closing it, using its widgets. A
//! part belongs to the client that opened it; its views and frames come to that client
//! only.

use serde::{Deserialize, Serialize};

use crate::plugin::UiEvent;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginRequest {
    /// Starts the plugin's interface when it is not running; its view follows as
    /// `Event::PluginView`.
    Open {
        plugin: String,
        part: String,
    },
    Close {
        plugin: String,
        part: String,
    },
    Event {
        plugin: String,
        part: String,
        widget: String,
        event: UiEvent,
    },
}
