//! Store covers: each asked of the core once, by its SHA-256, and kept for the session.
//! The core fetches a cover and checks it against the signed list; this only decodes it.

use std::{collections::HashMap, sync::Arc};

use anyhow::{Result, bail};
use base64::Engine;
use dusk_protocol::plugin::Listing;
use gpui_kit::*;

use crate::workspace::Workspace;

pub enum Cover {
    Loading,
    Ready(Arc<Image>),
    /// Not shown; the card keeps its letter.
    Failed,
}

#[derive(Default)]
pub struct Covers(pub HashMap<String, Cover>);

impl Covers {
    /// The listing's cover, once it has come.
    pub fn get(&self, listing: &Listing) -> Option<Arc<Image>> {
        let cover = listing.cover.as_ref()?;
        match self.0.get(&cover.sha256)? {
            Cover::Ready(image) => Some(Arc::clone(image)),
            Cover::Loading | Cover::Failed => None,
        }
    }
}

/// Only the two types a list may name are drawn.
fn decode(base64: &str, mime: &str) -> Result<Arc<Image>> {
    let format = match mime {
        "image/png" => ImageFormat::Png,
        "image/webp" => ImageFormat::Webp,
        other => bail!("a cover of type {other} is not drawn"),
    };
    let bytes = base64::engine::general_purpose::STANDARD.decode(base64)?;
    Ok(Arc::new(Image::from_bytes(format, bytes)))
}

impl Workspace {
    /// Asks for every cover of the list not asked for yet this session.
    pub(super) fn load_covers(&mut self, cx: &mut Context<Self>) {
        let mut wanted = Vec::new();
        let shop = &mut self.workshop;
        for listing in shop.listings.iter().flatten() {
            let Some(cover) = &listing.cover else {
                continue;
            };
            if shop.covers.0.contains_key(&cover.sha256) {
                continue;
            }
            shop.covers.0.insert(cover.sha256.clone(), Cover::Loading);
            wanted.push((
                cover.sha256.clone(),
                listing.source.clone(),
                listing.name.clone(),
            ));
        }
        for (sha, source, name) in wanted {
            self.core.call(
                cx,
                move |client| async move {
                    let (base64, mime) = client.plugins().cover(&source, &name).await?;
                    decode(&base64, &mime)
                },
                move |ws, result, cx| {
                    let cover = match result {
                        Ok(image) => Cover::Ready(image),
                        Err(error) => {
                            tracing::debug!("store cover not shown: {error:#}");
                            Cover::Failed
                        }
                    };
                    ws.workshop.covers.0.insert(sha, cover);
                    cx.notify();
                },
            );
        }
    }
}
