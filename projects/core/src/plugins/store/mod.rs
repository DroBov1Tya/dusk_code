//! The plugin store, the Workshop: signed lists on GitHub Pages, the official one and the
//! mirrors the user added, and the packages they name. Installing checks the list's
//! signature, serial and expiry, the package's size and SHA-256, unpacks it under the
//! rules of `unpack` and refuses a package whose manifest differs from the listing the
//! user agreed to. Only a plugin the store installed, by its record in the config folder,
//! is updated or removed by it, and only from the source that installed it; a folder the
//! user made is never touched.
//!
//! - `source` — the lists, read at once, and the tier each one earns;
//! - `index` — one list and its signature;
//! - `listing` — what a client is shown, and in which order;
//! - `fetch` — files over https to public addresses, capped and checked;
//! - `picture` — the covers and screenshots of the cards;
//! - `record` — what the store did here;
//! - `unpack` — a package into a folder;
//! - `content`, `pack` — skills, packs and agents, which are plain files;
//! - `contents` — the files of a listing, for a client to look through;
//! - `connector` — an MCP server a listing adds to the connectors, with no files;
//! - `guard` — what a source without a key may bring, and what names it may not take;
//! - `review` — what an install would write and change, asked before an Unverified one;
//! - `origin` — which skills and agents here came from an Unverified source.

pub mod add;
#[cfg(test)]
mod brings_tests;
pub mod connector;
#[cfg(test)]
mod connector_tests;
mod content;
#[cfg(test)]
mod content_tests;
pub mod contents;
#[cfg(test)]
mod contents_tests;
#[cfg(test)]
mod credit_tests;
mod fetch;
mod guard;
#[cfg(test)]
mod guard_tests;
pub mod index;
mod listing;
pub mod origin;
mod pack;
mod picture;
#[cfg(test)]
mod picture_tests;
mod record;
#[cfg(test)]
mod release_tests;
#[cfg(test)]
mod repositories_tests;
pub mod review;
mod source;
#[cfg(test)]
mod tests;
mod unpack;
mod upstream;
#[cfg(test)]
mod upstream_tests;
#[cfg(test)]
mod workshop_dist_tests;
#[cfg(test)]
mod workshop_tests;

use std::{fs, path::Path, sync::LazyLock};

use anyhow::{Context, Result, anyhow, ensure};
use dusk_protocol::plugin::{Kind, Order, StoreList, Tier, UpdateReport};
use tokio::sync::Mutex;

use super::{catalog, manifest::Manifest};
use crate::{config::Mirror, init::paths::Paths};
use fetch::Via;
use index::Entry;

pub use record::Installed;
pub(crate) use record::Record;
use source::Fetched;

pub use listing::{Here, taken_name};
pub use pack::{FILE as PACK_FILE, Pack};
pub use picture::{cover, screenshot};
pub use source::{probe, probed};

/// Where the store keeps a package while it is checked, under the plugins folder; the
/// leading dot keeps it out of the plugins found.
const STAGING: &str = ".staging";

/// One install or removal at a time: two of the same plugin would share its folder.
static BUSY: LazyLock<Mutex<()>> = LazyLock::new(Mutex::default);

/// What the sources offer, with what is installed here, in `order`; only `category`'s
/// when it is given, and only of `kinds` when they are. A source that cannot be read is
/// reported beside the rest.
pub async fn list(
    paths: &Paths,
    mirrors: &[Mirror],
    order: Order,
    category: Option<&str>,
    kinds: &[Kind],
    here: &Here,
) -> Result<StoreList> {
    let (fetched, failed) = source::read(paths, mirrors).await;
    let record = Record::load(paths)?;
    let mut listings: Vec<_> = fetched
        .iter()
        .flat_map(|source| {
            let record = &record;
            source
                .index
                .plugins
                .iter()
                .map(move |entry| listing::listing(paths, record, here, source, entry))
        })
        .chain(listing::local(paths, &record, here))
        .collect();
    listing::mark_clashes(&mut listings);
    let taken = fetched
        .iter()
        .filter(|source| source.tier == Tier::Unverified)
        .map(|source| {
            let names = listing::taken(paths, &record, &fetched, &source.name);
            (source.name.clone(), names)
        })
        .collect();
    listing::guard_names(&mut listings, &taken);
    listing::guard_agents(&mut listings);
    listings.retain(|listing| {
        (kinds.is_empty() || kinds.contains(&listing.kind))
            && category.is_none_or(|category| {
                listing.category == category
                    || listing.categories.iter().any(|item| item == category)
            })
    });
    listing::sort(&mut listings, order);
    let picks = listing::picks(&listings);
    Ok(StoreList {
        listings,
        failed,
        picks,
    })
}

/// The repositories the user added, each read now, with its state.
pub async fn sources(paths: &Paths, mirrors: &[Mirror]) -> Vec<dusk_protocol::plugin::SourceRow> {
    let (fetched, failed) = source::read(paths, mirrors).await;
    source::rows(mirrors, &fetched, &failed)
}

/// What the user agreed to install, as they were shown it.
pub struct Agreed<'a> {
    pub source: &'a str,
    pub tier: Tier,
    pub name: &'a str,
    pub version: &'a str,
    pub permissions: &'a [String],
    /// The `package_sha256` of the review the user saw; an Unverified install needs it.
    pub sha256: Option<&'a str>,
}

/// What an install did: files put in place, or a connector's server for the caller to
/// add to the config, as the settings add one, and then `connector::remember`.
pub enum Placed {
    Files(Kind),
    Server {
        config: crate::config::McpServerConfig,
        added: Box<Installed>,
    },
}

/// Installs or updates a plugin in the plugins folder, or a skill, pack or agent in the
/// rules folder, when its listing is still the one
/// the user agreed to: the same source and tier, version and permissions. A connector
/// writes nothing here: its server comes back to be added.
/// Answers with what it installed.
pub async fn install(paths: &Paths, mirrors: &[Mirror], agreed: &Agreed<'_>) -> Result<Placed> {
    let _busy = BUSY.lock().await;
    // Every source, since a mirror's tier depends on the official list.
    let (fetched, failed) = source::read(paths, mirrors).await;
    if let Some(failure) = failed.iter().find(|item| item.source == agreed.source) {
        return Err(anyhow!("{}: {}", failure.source, failure.error));
    }
    let (source, entry) = pick(&fetched, agreed)?;
    if let Some(why) = listing::reserved(&source.name, entry.kind, &entry.name) {
        anyhow::bail!(why);
    }
    if source.tier == Tier::Unverified {
        let record = Record::load(paths)?;
        let taken = listing::taken(paths, &record, &fetched, &source.name);
        if let Some(why) = guard::shadow(&entry.name, taken.iter().map(String::as_str)) {
            anyhow::bail!(why);
        }
    }
    if entry.kind == Kind::Connector {
        let server = entry
            .server
            .as_ref()
            .context("a connector names no server")?;
        let tier = listing::tier(source, entry);
        if let Some(why) = server.unfit(&entry.name, tier, entry.badge) {
            anyhow::bail!("{} cannot be added: {why}", entry.name);
        }
        if let Some(url) = &server.url {
            connector::resolves_publicly(url).await?;
        }
        if let Some(release) = &server.release {
            connector::install::install(paths, &entry.name, release).await?;
        }
        let config = server.config(paths, &entry.name);
        let mut added = Installed::new(source, &entry.version, "");
        added.kind = Kind::Connector;
        added.target = connector::target(&config);
        added.tier = Some(tier);
        added.author = entry.author.clone().unwrap_or_default();
        added.origin = entry.source.clone().unwrap_or_default();
        return Ok(Placed::Server {
            config,
            added: Box::new(added),
        });
    }
    put(paths, source, entry)
        .await
        .map(|()| Placed::Files(entry.kind))
}

/// The source and entry the user agreed to, refused when anything they were shown has
/// changed since.
pub fn pick<'a>(fetched: &'a [Fetched], agreed: &Agreed<'_>) -> Result<(&'a Fetched, &'a Entry)> {
    let name = agreed.name;
    let source = fetched
        .iter()
        .find(|item| item.name == agreed.source)
        .with_context(|| format!("there is no source named {}", agreed.source))?;
    let entry = source
        .index
        .plugins
        .iter()
        .find(|entry| entry.name == name)
        .with_context(|| format!("{} offers no {name}", source.name))?;
    let tier = listing::tier(source, entry);
    ensure!(
        tier == agreed.tier,
        "{name} of {} is {tier:?} now, not {:?} as you saw it; look at the list again",
        source.name,
        agreed.tier
    );
    ensure!(
        entry.version == agreed.version && entry.consent(tier) == agreed.permissions,
        "the listing of {name} changed since you saw it; look at the list again"
    );
    // Nothing signed an Unverified list, so the files the user looked at are what they
    // agreed to, not the version: the list may name other files under the same one. A
    // connector has no files; its address is in what they agreed to.
    if tier == Tier::Unverified && entry.kind != Kind::Connector {
        let pinned = entry
            .upstream
            .as_ref()
            .map(|upstream| upstream.digest(entry));
        ensure!(
            agreed.sha256.is_some() && agreed.sha256 == pinned.as_deref(),
            "the files of {name} are not the ones you were shown; look at them again"
        );
    }
    Ok((source, entry))
}

/// Updates every plugin a source installed to that source's newer version, unless the
/// update asks for more than the version here, or the source is no longer the one that
/// installed it: the user decides those by hand. Other installs may run between two
/// updates.
pub async fn update_all(paths: &Paths, mirrors: &[Mirror]) -> Result<UpdateReport> {
    let (fetched, failed) = source::read(paths, mirrors).await;
    update_from(paths, &fetched, &failed).await
}

/// `update_all` with the lists read. Nothing of an Unverified source is updated here:
/// the user updates it by hand once they saw what changed (`review`).
async fn update_from(
    paths: &Paths,
    fetched: &[Fetched],
    failed: &[dusk_protocol::plugin::SourceFailure],
) -> Result<UpdateReport> {
    let names: Vec<String> = Record::load(paths)?.installed.into_keys().collect();
    let mut report = UpdateReport::default();
    for name in names {
        let _busy = BUSY.lock().await;
        let record = Record::load(paths)?;
        let Some(installed) = record.present(paths, &name) else {
            continue;
        };
        let Some(source) = fetched.iter().find(|item| item.name == installed.source) else {
            if let Some(failure) = failed.iter().find(|item| item.source == installed.source) {
                report.skipped.push((name.clone(), failure.error.clone()));
            }
            continue;
        };
        if !installed.is_from(source) {
            let why = format!("the mirror {} changed; reinstall it", source.name);
            report.skipped.push((name.clone(), why));
            continue;
        }
        let Some(entry) = source.index.plugins.iter().find(|item| item.name == name) else {
            continue;
        };
        if listing::newer(&entry.version, &installed.version).is_none() {
            continue;
        }
        if source.tier == Tier::Unverified {
            let why = "from an unverified source; update it by hand once you saw what changed";
            report.skipped.push((name.clone(), why.to_owned()));
            continue;
        }
        let folder = catalog::folder(paths).join(&name);
        // Skills, packs and agents ask for nothing; a list saying they do is refused.
        let more = match entry.kind {
            Kind::Plugin => asks_more(&folder, entry),
            _ => Ok(false),
        };
        let outcome = match more {
            Ok(false) => put(paths, source, entry).await,
            Ok(true) => Err(anyhow!(
                "{} asks for more than the version here; update it by hand",
                entry.version
            )),
            Err(error) => Err(error),
        };
        match outcome {
            Ok(()) => report.updated.push(format!("{name} {}", entry.version)),
            Err(error) => report.skipped.push((name.clone(), format!("{error:#}"))),
        }
    }
    Ok(report)
}

/// The single skills the store installed, by name: their folders are under `skills/` like
/// the user's own, so only the record tells them apart.
pub fn installed_skills(paths: &Paths) -> std::collections::BTreeSet<String> {
    let record = Record::load(paths).unwrap_or_default();
    record
        .installed
        .iter()
        .filter(|(name, item)| item.kind == Kind::Skill && record.present(paths, name).is_some())
        .map(|(name, _)| name.clone())
        .collect()
}

/// What the store installed as `name`, when it may remove it: it installed it and its
/// folder is there. Asked before anything changes.
pub fn check_removable(paths: &Paths, name: &str) -> Result<Kind> {
    ensure!(
        crate::init::paths::is_plain_folder_name(name),
        "invalid name"
    );
    Record::load(paths)?
        .present(paths, name)
        .map(|installed| installed.kind)
        .with_context(|| {
            format!("{name} was not installed from the store; remove its folder yourself")
        })
}

/// Removes a plugin's folder whole; of a skill, pack or agent only the files the store
/// wrote and the user did not edit since.
pub async fn remove(paths: &Paths, name: &str) -> Result<()> {
    let _busy = BUSY.lock().await;
    let kind = check_removable(paths, name)?;
    match kind {
        Kind::Plugin => {
            let folder = catalog::folder(paths).join(name);
            fs::remove_dir_all(&folder)
                .with_context(|| format!("cannot remove {}", folder.display()))?;
        }
        _ => content::remove(paths, name)?,
    }
    Record::update(paths, |record| {
        let Some(item) = record.installed.remove(name) else {
            return Ok(());
        };
        // Files the user edited stay; they are still that source's, not the user's own.
        let stays = kind != Kind::Plugin
            && item.source != dusk_protocol::plugin::OFFICIAL_SOURCE
            && item.path(paths, name).is_dir();
        if stays {
            tracing::info!(
                "{name} removed; the files you edited stay in {}, still marked as from {}",
                item.folder,
                item.source
            );
            let left = record::Left {
                source: item.source.clone(),
                unverified: item.unverified(),
            };
            record.left.insert(item.folder.clone(), left);
        }
        Ok(())
    })
}

/// Downloads, checks and places `entry` from `source`; called under `BUSY`.
async fn put(paths: &Paths, source: &Fetched, entry: &Entry) -> Result<()> {
    let name = &entry.name;
    let owner = Record::load(paths)?.installed.get(name).cloned();
    if let Some(owner) = &owner {
        ensure!(
            owner.kind == entry.kind,
            "{name} is installed here as a {:?}, the list offers a {:?}",
            owner.kind,
            entry.kind
        );
        ensure!(
            owner.source == source.name,
            "{name} was installed from {}; remove it before installing it from {}",
            owner.source,
            source.name
        );
        ensure!(
            owner.is_from(source),
            "the mirror {} changed its address or key since it installed {name}; \
             remove {name} and install it again",
            source.name
        );
    }
    let (package, sha256) = fetch_package(entry).await?;
    let placed = entry.clone();
    let folder = match entry.kind {
        Kind::Plugin => {
            let Package::Archive(package) = package else {
                anyhow::bail!("a plugin comes only as a package");
            };
            let root = catalog::folder(paths);
            let owned = owner.is_some();
            tokio::task::spawn_blocking(move || place(&root, &placed, &package, owned))
                .await
                .context("the install stopped")?
                .with_context(|| format!("cannot install {name}"))?;
            String::new()
        }
        _ => {
            let paths = paths.clone();
            let owned = owner.map(|owner| owner.folder);
            let unverified = source.tier == Tier::Unverified;
            tokio::task::spawn_blocking(move || {
                crate::init::create_private_dir(&catalog::folder(&paths))?;
                content::place_package(&paths, &placed, &package, owned.as_deref(), unverified)
            })
            .await
            .context("the install stopped")?
            .with_context(|| format!("cannot install {name}"))?
        }
    };
    let mut installed = Installed::new(source, &entry.version, &sha256);
    installed.kind = entry.kind;
    installed.folder = folder;
    installed.author = entry.author.clone().unwrap_or_default();
    installed.license = entry.license.clone().unwrap_or_default();
    installed.origin = match &entry.upstream {
        Some(upstream) => format!(
            "https://github.com/{}/tree/{}",
            upstream.repo, upstream.commit
        ),
        None => entry.source.clone().unwrap_or_default(),
    };
    Record::update(paths, |record| {
        record.left.remove(&installed.folder);
        record.installed.insert(name.clone(), installed);
        Ok(())
    })
}

/// The package of `entry`, or its files pinned upstream, fetched and checked, with the
/// hash the record keeps of it.
async fn fetch_package(entry: &Entry) -> Result<(Package, String)> {
    let name = &entry.name;
    Ok(match &entry.upstream {
        Some(upstream) => {
            let files = upstream::fetch_all(upstream)
                .await
                .with_context(|| format!("the files of {name}"))?;
            (Package::Files(files), upstream.digest(entry))
        }
        None => {
            let download = entry.download_here().map_err(anyhow::Error::msg)?;
            let bytes =
                fetch::fetch_checked(&download.url, download.size, &download.sha256, Via::Package)
                    .await
                    .with_context(|| format!("the package of {name}"))?;
            (Package::Archive(bytes), download.sha256.clone())
        }
    })
}

/// What an install writes: a checked package, or files checked one by one.
pub enum Package {
    Archive(Vec<u8>),
    Files(std::collections::BTreeMap<String, Vec<u8>>),
}

/// Whether `entry` asks for a permission or a model provider the plugin in `folder` does
/// not have, or brings an MCP server or a program it does not.
fn asks_more(folder: &Path, entry: &Entry) -> Result<bool> {
    let manifest = Manifest::load(folder)?;
    let (servers, programs) = (servers(&manifest), manifest.programs());
    if entry.mcp.iter().any(|server| !servers.contains(server))
        || entry.programs.iter().any(|line| !programs.contains(line))
    {
        return Ok(true);
    }
    let had: Vec<&str> = manifest
        .providers
        .values()
        .map(|provider| provider.base_url.as_str())
        .collect();
    Ok(!catalog::covers(&manifest.permissions, &entry.permissions)
        || entry
            .providers
            .iter()
            .any(|url| !had.contains(&url.as_str())))
}

/// Unpacks the checked package beside the plugins, checks its manifest against the list
/// and swaps it in; a folder there is replaced only when the store `owned` it, and comes
/// back when the swap fails.
fn place(root: &Path, entry: &Entry, package: &[u8], owned: bool) -> Result<()> {
    let staging = root.join(STAGING);
    // Under `BUSY`, so what is here was left by an install that stopped.
    if fs::symlink_metadata(&staging).is_ok() {
        fs::remove_dir_all(&staging)?;
    }
    crate::init::create_private_dir(&staging)?;
    let fresh = staging.join(&entry.name);
    let old = staging.join(format!("{}.old", entry.name));
    let target = root.join(&entry.name);
    let result = (|| {
        unpack::unpack(package, &fresh)?;
        check_manifest(&fresh, entry)?;
        if fs::symlink_metadata(&target).is_ok() {
            ensure!(
                owned,
                "a plugin named {} is already here and not from the store",
                entry.name
            );
            fs::rename(&target, &old)?;
        }
        if let Err(error) = fs::rename(&fresh, &target) {
            if fs::symlink_metadata(&old).is_ok() {
                fs::rename(&old, &target).context("cannot put the old version back")?;
            }
            return Err(error.into());
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}

/// The package's manifest must say what the list showed the user.
fn check_manifest(folder: &Path, entry: &Entry) -> Result<()> {
    let manifest = Manifest::load(folder)?;
    ensure!(
        manifest.version == entry.version,
        "the package is version {}, the list says {}",
        manifest.version,
        entry.version
    );
    ensure!(
        manifest.permissions == entry.permissions,
        "the package asks for other permissions than the list shows"
    );
    ensure!(
        manifest.platforms == entry.platforms && manifest.clients == entry.clients,
        "the package names other systems or clients than the list"
    );
    let mut providers: Vec<&str> = manifest
        .providers
        .values()
        .map(|provider| provider.base_url.as_str())
        .collect();
    providers.sort_unstable();
    ensure!(
        providers == entry.providers,
        "the package brings other model providers than the list shows"
    );
    // A list before format 3 cannot say what a plugin runs; the user was shown nothing
    // of it, as before.
    if entry.format >= 3 {
        let mut listed = entry.mcp.clone();
        listed.sort();
        ensure!(
            servers(&manifest) == listed,
            "the package brings other MCP servers than the list shows"
        );
        ensure!(
            manifest.programs() == entry.programs,
            "the package runs other programs than the list shows"
        );
        let listed: std::collections::BTreeSet<&str> =
            entry.skills.iter().map(String::as_str).collect();
        let held = skill_folders(folder)?;
        ensure!(
            held.iter()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>()
                == listed,
            "the package holds other skills than the list shows"
        );
    }
    Ok(())
}

/// A manifest's MCP servers as a list names them, sorted by name.
pub fn servers(manifest: &Manifest) -> Vec<index::Server> {
    use crate::config::McpServerConfig;
    manifest
        .mcp
        .iter()
        .map(|(name, server)| match server {
            McpServerConfig::Stdio(stdio) => index::Server {
                name: name.clone(),
                command: Some(stdio.command.clone()),
                args: stdio.args.clone(),
                url: None,
            },
            McpServerConfig::Http(http) => index::Server {
                name: name.clone(),
                command: None,
                args: Vec::new(),
                url: Some(http.url.clone()),
            },
        })
        .collect()
}

/// The folders under a plugin's `skills/`, each a skill or a set of them.
fn skill_folders(folder: &Path) -> Result<Vec<String>> {
    let skills = folder.join("skills");
    if fs::symlink_metadata(&skills).is_err() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for item in fs::read_dir(&skills).context("cannot list its skills")? {
        let item = item?;
        if item.file_type()?.is_dir() {
            names.push(
                item.file_name()
                    .to_str()
                    .context("a skill's name is not UTF-8")?
                    .to_owned(),
            );
        }
    }
    Ok(names)
}
