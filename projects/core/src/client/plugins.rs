//! The parts plugins add to the window, for a client that draws them.

use super::Client;
use anyhow::{Result, bail};
use dusk_protocol::{
    client::{PluginRequest, Request, Response},
    plugin::{
        Contents, FilePreview, Kind, Listing, Order, Probe, Review, SourceRow, StoreList,
        StoreRequest, UiEvent, UpdateReport,
    },
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
    /// The listings in `order`, only `category`'s when one is given and only of `kinds`
    /// when they are, and the sources that could not be read; a client shows both.
    pub async fn store(
        &self,
        order: Order,
        category: Option<String>,
        kinds: Vec<Kind>,
    ) -> Result<StoreList> {
        let request = StoreRequest::List {
            order,
            category,
            kinds,
        };
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
    pub async fn screenshot(
        &self,
        source: &str,
        name: &str,
        index: u32,
    ) -> Result<(String, String)> {
        let request = StoreRequest::Screenshot {
            source: source.to_owned(),
            name: name.to_owned(),
            index,
        };
        self.media(request).await
    }

    /// The files of `name` from `source`: the copy here, or the package checked by the
    /// core, which may fetch it first.
    pub async fn contents(&self, source: &str, name: &str) -> Result<Contents> {
        let request = StoreRequest::Contents {
            source: source.to_owned(),
            name: name.to_owned(),
        };
        match self.0.call(Request::Store(request)).await? {
            Response::StoreContents(contents) => Ok(contents),
            Response::Error(error) => bail!(error),
            other => bail!("unexpected answer: {other:?}"),
        }
    }

    /// One text file of the last `contents` of `name`.
    pub async fn contents_file(&self, source: &str, name: &str, path: &str) -> Result<FilePreview> {
        let request = StoreRequest::File {
            source: source.to_owned(),
            name: name.to_owned(),
            path: path.to_owned(),
        };
        match self.0.call(Request::Store(request)).await? {
            Response::StoreFile(file) => Ok(file),
            Response::Error(error) => bail!(error),
            other => bail!("unexpected answer: {other:?}"),
        }
    }

    /// What the signed list at `url` holds with `key`; nothing is saved.
    pub async fn probe_source(&self, url: &str, key: &str) -> Result<Probe> {
        let request = StoreRequest::Probe {
            url: url.to_owned(),
            key: key.to_owned(),
        };
        match self.0.call(Request::Store(request)).await? {
            Response::StoreProbe(probe) => Ok(probe),
            Response::Error(error) => bail!(error),
            other => bail!("unexpected answer: {other:?}"),
        }
    }

    /// The repositories the user added, each read now.
    pub async fn sources(&self) -> Result<Vec<SourceRow>> {
        match self.0.call(Request::Store(StoreRequest::Sources)).await? {
            Response::StoreSources(rows) => Ok(rows),
            Response::Error(error) => bail!(error),
            other => bail!("unexpected answer: {other:?}"),
        }
    }

    /// Adds a repository once the core read its list with `key`.
    pub async fn add_source(&self, url: &str, key: &str) -> Result<()> {
        let request = StoreRequest::AddSource {
            url: url.to_owned(),
            key: key.to_owned(),
        };
        self.0.done(Request::Store(request)).await
    }

    pub async fn remove_source(&self, name: &str) -> Result<()> {
        let request = StoreRequest::RemoveSource {
            name: name.to_owned(),
        };
        self.0.done(Request::Store(request)).await
    }

    async fn media(&self, request: StoreRequest) -> Result<(String, String)> {
        match self.0.call(Request::Store(request)).await? {
            Response::Media { base64, mime } => Ok((base64, mime)),
            Response::Error(error) => bail!(error),
            other => bail!("unexpected answer: {other:?}"),
        }
    }

    /// Installs or updates the plugin of `listing`, as the user saw it, and switches it
    /// on; refused when the store's listing changed since. `sha256` is the
    /// `package_sha256` of the review the user saw, which an Unverified listing needs.
    pub async fn install(&self, listing: &Listing, sha256: Option<&str>) -> Result<()> {
        let request = StoreRequest::Install {
            source: listing.source.clone(),
            tier: listing.tier,
            name: listing.name.clone(),
            version: listing.version.clone(),
            agreed: listing.permissions.clone(),
            sha256: sha256.map(str::to_owned),
        };
        self.0.done(Request::Store(request)).await
    }

    /// What installing `name` from `source` would write and change, checked by the core
    /// without writing it; shown before an Unverified listing is installed or updated.
    pub async fn review(&self, source: &str, name: &str) -> Result<Review> {
        let request = StoreRequest::Review {
            source: source.to_owned(),
            name: name.to_owned(),
        };
        match self.0.call(Request::Store(request)).await? {
            Response::StoreReview(review) => Ok(review),
            Response::Error(error) => bail!(error),
            other => bail!("unexpected answer: {other:?}"),
        }
    }

    pub async fn remove(&self, name: &str) -> Result<()> {
        let request = StoreRequest::Remove {
            name: name.to_owned(),
        };
        self.0.done(Request::Store(request)).await
    }
}
