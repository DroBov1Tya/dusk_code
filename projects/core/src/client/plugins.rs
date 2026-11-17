//! The parts plugins add to the window, for a client that draws them.

use super::Client;
use anyhow::{Result, bail};
use dusk_protocol::{
    client::{PluginRequest, Request, Response},
    plugin::{Listing, Order, StoreRequest, UiEvent},
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

    pub async fn redetect(&self, plugin: &str, part: &str) -> Result<()> {
        let request = PluginRequest::Redetect {
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

/// The plugin store. The core checks the signed list and every package; a client shows
/// the list and asks the user before `install`, showing the listing's permissions.
impl Plugins<'_> {
    /// The listings in trust order. A source that cannot be read is logged; when none
    /// can, the first one's error is the answer.
    pub async fn store(&self) -> Result<Vec<Listing>> {
        let request = StoreRequest::List {
            order: Order::default(),
            category: None,
        };
        let list = match self.0.call(Request::Store(request)).await? {
            Response::Store(list) => list,
            Response::Error(error) => bail!(error),
            other => bail!("unexpected answer: {other:?}"),
        };
        for failure in &list.failed {
            tracing::warn!("store source {}: {}", failure.source, failure.error);
        }
        match (list.listings.is_empty(), list.failed.first()) {
            (true, Some(failure)) => bail!("{}: {}", failure.source, failure.error),
            _ => Ok(list.listings),
        }
    }

    /// Installs or updates the plugin of `listing`, as the user saw it, and switches it
    /// on; refused when the store's listing changed since.
    pub async fn install(&self, listing: &Listing) -> Result<()> {
        let request = StoreRequest::Install {
            source: listing.source.clone(),
            tier: listing.tier,
            name: listing.name.clone(),
            version: listing.version.clone(),
            agreed: listing.permissions.clone(),
        };
        self.0.done(Request::Store(request)).await
    }

    pub async fn remove(&self, name: &str) -> Result<()> {
        let request = StoreRequest::Remove {
            name: name.to_owned(),
        };
        self.0.done(Request::Store(request)).await
    }
}
