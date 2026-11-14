//! The parts plugins add to the window, for a client that draws them.

use super::Client;
use anyhow::Result;
use dusk_protocol::{
    client::{PluginRequest, Request},
    plugin::UiEvent,
};

/// Views, frames and notes of the parts this client opened come as `Event::PluginView`,
/// `Event::PluginFrame` and `Event::PluginNotice`.
pub struct Plugins<'a>(pub(super) &'a Client);

impl Plugins<'_> {
    pub async fn open(&self, plugin: &str, part: &str) -> Result<()> {
        let request = PluginRequest::Open {
            plugin: plugin.to_owned(),
            part: part.to_owned(),
        };
        self.0.done(Request::Plugin(request)).await
    }

    pub async fn close(&self, plugin: &str, part: &str) -> Result<()> {
        let request = PluginRequest::Close {
            plugin: plugin.to_owned(),
            part: part.to_owned(),
        };
        self.0.done(Request::Plugin(request)).await
    }

    pub async fn event(
        &self,
        plugin: &str,
        part: &str,
        widget: &str,
        event: UiEvent,
    ) -> Result<()> {
        let request = PluginRequest::Event {
            plugin: plugin.to_owned(),
            part: part.to_owned(),
            widget: widget.to_owned(),
            event,
        };
        self.0.done(Request::Plugin(request)).await
    }
}
