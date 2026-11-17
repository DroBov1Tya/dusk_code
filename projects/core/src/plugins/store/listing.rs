//! What a client is shown: an entry of a checked list as a listing, the plugins found
//! here that no source installed, whether an update is offered, and the order, decided
//! here so every client shows the same.

use std::cmp::Reverse;

use chrono::{DateTime, FixedOffset};
use dusk_protocol::plugin::{Kind, LOCAL_SOURCE, Listing, Order, Tier};

use super::{index::Entry, record::Record, source::Fetched};
use crate::{
    init::paths::Paths,
    plugins::{catalog, manifest::Manifest},
};

pub fn listing(paths: &Paths, record: &Record, source: &Fetched, entry: &Entry) -> Listing {
    let installed = record
        .installed_from(paths, source, &entry.name)
        .map(|item| item.version.clone());
    Listing {
        name: entry.name.clone(),
        version: entry.version.clone(),
        description: entry.description.clone(),
        platforms: entry.platforms.clone(),
        clients: entry.clients.clone(),
        permissions: entry.consent(),
        homepage: entry.homepage.clone(),
        update: installed
            .as_deref()
            .and_then(|installed| newer(&entry.version, installed)),
        installed,
        unfit: entry.download_here().err(),
        tier: source.tier,
        source: source.name.clone(),
        kind: entry.kind,
        category: entry.category.clone(),
        tags: entry.tags.clone(),
        cover: entry
            .cover
            .as_ref()
            .map(|cover| dusk_protocol::plugin::Cover {
                path: cover.path.clone(),
                sha256: cover.sha256.clone(),
                size: cover.size,
            }),
        screenshots: entry
            .screenshots
            .iter()
            .map(|screenshot| dusk_protocol::plugin::ScreenshotRef {
                caption: screenshot.caption.clone(),
            })
            .collect(),
        stars: entry.stars,
        created: entry.created.clone(),
        updated: entry.updated.clone(),
        skills: entry.skills.clone(),
        clash: false,
    }
}

/// Marks every listing whose name another source offers too.
pub fn mark_clashes(listings: &mut [Listing]) {
    let mut sources = std::collections::BTreeMap::<String, usize>::new();
    for listing in listings.iter() {
        *sources.entry(listing.name.clone()).or_default() += 1;
    }
    for listing in listings {
        listing.clash = sources[&listing.name] > 1;
    }
}

/// The plugins in the plugins folder that no source installed: shipped with duskcode or
/// put there by the user. Shown, never updated or removed by the store.
pub fn local(paths: &Paths, record: &Record) -> Vec<Listing> {
    catalog::plugin_folders(&catalog::folder(paths))
        .into_iter()
        .filter_map(|folder| {
            let name = folder.file_name()?.to_str()?.to_owned();
            if record.installed.contains_key(&name) {
                return None;
            }
            let manifest = Manifest::load(&folder)
                .inspect_err(|error| tracing::debug!("plugin {name} not listed: {error:#}"))
                .ok()?;
            Some(Listing {
                name,
                description: manifest.description,
                platforms: manifest.platforms,
                clients: manifest.clients,
                permissions: manifest.permissions.lines(),
                installed: Some(manifest.version.clone()),
                version: manifest.version,
                tier: Tier::Local,
                source: LOCAL_SOURCE.to_owned(),
                kind: Kind::Plugin,
                ..Listing::default()
            })
        })
        .collect()
}

/// `available`, when it is a strictly greater semantic version than `installed`. A
/// version that is not one offers no update rather than a guess; build metadata does
/// not count, as semver says.
pub fn newer(available: &str, installed: &str) -> Option<String> {
    let parse = |text: &str| {
        let mut version = semver::Version::parse(text).ok()?;
        version.build = semver::BuildMetadata::EMPTY;
        Some(version)
    };
    (parse(available)? > parse(installed)?).then(|| available.to_owned())
}

/// Sorts by `order`; ties, and listings without the date or count sorted on, keep the
/// trust order: Official, Verified, Community, Local, then by name.
pub fn sort(listings: &mut [Listing], order: Order) {
    let trust = |listing: &Listing| (listing.tier, listing.name.clone(), listing.source.clone());
    match order {
        Order::Trust => listings.sort_by_key(trust),
        Order::Newest => listings.sort_by_key(|item| (Reverse(date(&item.created)), trust(item))),
        Order::Updated => listings.sort_by_key(|item| (Reverse(date(&item.updated)), trust(item))),
        Order::Stars => listings.sort_by_key(|item| (Reverse(item.stars), trust(item))),
    }
}

/// None sorts after every date once reversed.
fn date(text: &Option<String>) -> Option<DateTime<FixedOffset>> {
    text.as_deref()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
}
