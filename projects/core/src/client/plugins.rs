//! The parts plugins add to the window, for a client that draws them.

use super::Client;
use anyhow::{Result, bail};
use dusk_protocol::{
    client::{PluginRequest, Request, Response},
    plugin::{Listing, Order, StoreList, StoreRequest, UiEvent, UpdateReport},
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
    /// The listings in `order`, only `category`'s when one is given, and the sources
    /// that could not be read; a client shows both.
    pub async fn store(&self, order: Order, category: Option<String>) -> Result<StoreList> {
        let request = StoreRequest::List { order, category };
        let list = match self.0.call(Request::Store(request)).await? {
            Response::Store(list) => list,
            Response::Error(error) => bail!(error),
            other => bail!("unexpected answer: {other:?}"),
        };
        for failure in &list.failed {
            tracing::warn!("store source {}: {}", failure.source, failure.error);
        }
        Ok(list)
    }

    /// Updates every installed listing whose update asks for nothing more.
    pub async fn update_all(&self) -> Result<UpdateReport> {
        match self.0.call(Request::Store(StoreRequest::UpdateAll)).await? {
            Response::StoreUpdates(report) => Ok(report),
            Response::Error(error) => bail!(error),
            other => bail!("unexpected answer: {other:?}"),
        }
    }

    /// The cover of `name` from `source`, checked by the core against its list: the
    /// bytes in base64 and their type.
    pub async fn cover(&self, source: &str, name: &str) -> Result<(String, String)> {
        let request = StoreRequest::Cover {
            source: source.to_owned(),
            name: name.to_owned(),
        };
        self.media(request).await
    }

    /// The `index`th screenshot of `name` from `source`, checked like a cover.
    pub async fn screenshot(&self, source: &str, name: &str, index: u32) -> Result<(String, String)> {
        let request = StoreRequest::Screenshot {
            source: source.to_owned(),
            name: name.to_owned(),
            index,
        };
        self.media(request).await
    }

    async fn media(&self, request: StoreRequest) -> Result<(String, String)> {
        match self.0.call(Request::Store(request)).await? {
            Response::Media { base64, mime } => Ok((base64, mime)),
            Response::Error(error) => bail!(error),
            other => bail!("unexpected answer: {other:?}"),
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
