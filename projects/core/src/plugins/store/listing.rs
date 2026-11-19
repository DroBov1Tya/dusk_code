//! What a client is shown: an entry of a checked list as a listing, the plugins found
//! here that no source installed, whether an update is offered, and the order, decided
//! here so every client shows the same.

use std::cmp::Reverse;

use chrono::{DateTime, FixedOffset};
use std::collections::BTreeSet;

use dusk_protocol::plugin::{
    Connector, ConnectorState, Kind, LOCAL_SOURCE, Listing, Order, PICKS, Picks, RepoStars, Runs,
    Tier,
};

use super::{
    index::{Badge, Entry, Server},
    record::Record,
    source::Fetched,
};
use crate::{
    init::paths::Paths,
    plugins::{catalog, manifest::Manifest},
};

/// What is set up here, for the state of a plugin's connectors.
#[derive(Debug, Default)]
pub struct Here {
    /// The MCP servers of the user's config, by name; a plugin's of the same name is
    /// never used.
    pub yours: BTreeSet<String>,
    /// The plugins that are on.
    pub on: BTreeSet<String>,
}

impl Here {
    fn connectors(&self, plugin: &str, installed: bool, servers: &[Server]) -> Vec<Connector> {
        servers
            .iter()
            .map(|server| Connector {
                name: server.name.clone(),
                runs: match &server.url {
                    Some(url) => Runs::Url(url.clone()),
                    None => Runs::Command(server.line()),
                },
                state: if self.yours.contains(&server.name) {
                    ConnectorState::Yours
                } else if installed && self.on.contains(plugin) {
                    ConnectorState::Added
                } else {
                    ConnectorState::NotAdded
                },
            })
            .collect()
    }
}

pub fn listing(
    paths: &Paths,
    record: &Record,
    here: &Here,
    source: &Fetched,
    entry: &Entry,
) -> Listing {
    let installed = record
        .installed_from(paths, source, &entry.name)
        .map(|item| item.version.clone());
    Listing {
        connectors: here.connectors(&entry.name, installed.is_some(), &entry.mcp),
        programs: entry.programs.clone(),
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
        tier: tier(source, entry),
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
        repo_stars: repo_stars(source.tier, entry),
        created: entry.created.clone(),
        updated: entry.updated.clone(),
        skills: entry.skills.clone(),
        author: entry.author.clone().unwrap_or_default(),
        license: entry.license.clone().unwrap_or_default(),
        origin: entry.source.clone(),
        upstream: entry.upstream.as_ref().map(super::upstream::Upstream::line),
        categories: entry.categories.clone(),
        clash: false,
    }
}

/// Who the Official mark is for: the maintainers of the official Workshop, as its
/// publisher (`scripts/repo.py`) names them.
const OWNERS: [&str; 2] = ["duskcode", "DroBov1Tya"];

/// How far `entry` is trusted: its source's tier, lowered to Verified for an entry of the
/// official list that somebody else made. Only the official list's badge and author are
/// read, and only to lower: a mirror's list cannot raise its own trust by what it says.
pub fn tier(source: &Fetched, entry: &Entry) -> Tier {
    let not_ours = entry.badge == Some(Badge::Verified)
        || entry
            .author
            .as_deref()
            .is_some_and(|author| !OWNERS.contains(&author));
    if source.tier == Tier::Official && not_ours {
        Tier::Verified
    } else {
        source.tier
    }
}

/// The stars a listing may show: only a mirror the user added, and only with the GitHub
/// repository its `homepage` names, since the count is that repository's. The official
/// list's and a verified mirror's counts were the store's or a vendor's whole
/// repository's, so they are dropped whatever a list says.
fn repo_stars(tier: Tier, entry: &Entry) -> Option<RepoStars> {
    if tier != Tier::Community || entry.stars == 0 {
        return None;
    }
    Some(RepoStars {
        repo: github_repo(entry.homepage.as_deref()?)?,
        stars: entry.stars,
    })
}

/// `owner/name` of a `https://github.com/owner/name[/...]` page; nothing for any other
/// page or for a name GitHub would not allow, so the text shown stays a plain name.
fn github_repo(page: &str) -> Option<String> {
    let rest = page.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }
    let path = path.split(['?', '#']).next()?;
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    let owner_ok = (1..=39).contains(&owner.len())
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-');
    let name_ok = (1..=100).contains(&name.len())
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    (owner_ok && name_ok).then(|| format!("{owner}/{name}"))
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
pub fn local(paths: &Paths, record: &Record, here: &Here) -> Vec<Listing> {
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
            let programs = manifest.programs();
            let servers = super::servers(&manifest);
            let connectors = here.connectors(&name, true, &servers);
            let mut permissions = super::index::brings(&programs, &servers);
            permissions.extend(manifest.permissions.lines());
            Some(Listing {
                name,
                connectors,
                programs,
                description: manifest.description,
                platforms: manifest.platforms,
                clients: manifest.clients,
                permissions,
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

/// Sorts by `order`; ties, and listings without the date sorted on, keep the
/// trust order: Official, Verified, Community, Local, then by name.
pub fn sort(listings: &mut [Listing], order: Order) {
    let trust = |listing: &Listing| (listing.tier, listing.name.clone(), listing.source.clone());
    match order {
        Order::Trust => listings.sort_by_key(trust),
        Order::Newest => listings.sort_by_key(|item| (Reverse(date(&item.created)), trust(item))),
        Order::Updated => listings.sort_by_key(|item| (Reverse(date(&item.updated)), trust(item))),
    }
}

/// The shelves of `listings`: the first few by date, each an index into `listings`, so
/// a client shows them without sorting anything itself. None ranks by stars: a mirror
/// writes its own count.
pub fn picks(listings: &[Listing]) -> Picks {
    let top = |order: Order, has: fn(&Listing) -> bool| -> Vec<usize> {
        let mut sorted: Vec<Listing> = listings.iter().filter(|item| has(item)).cloned().collect();
        sort(&mut sorted, order);
        sorted
            .iter()
            .take(PICKS)
            .filter_map(|item| {
                listings
                    .iter()
                    .position(|other| other.source == item.source && other.name == item.name)
            })
            .collect()
    };
    Picks {
        newest: top(Order::Newest, |item| date(&item.created).is_some()),
        updated: top(Order::Updated, |item| date(&item.updated).is_some()),
    }
}

/// None sorts after every date once reversed.
fn date(text: &Option<String>) -> Option<DateTime<FixedOffset>> {
    text.as_deref()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
}
