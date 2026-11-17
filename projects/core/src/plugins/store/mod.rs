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
//! - `cover` — the pictures of the cards;
//! - `record` — what the store did here;
//! - `unpack` — a package into a folder;
//! - `content`, `pack` — skills, packs and agents, which are plain files.

mod content;
#[cfg(test)]
mod content_tests;
mod cover;
mod fetch;
pub mod index;
mod listing;
mod pack;
mod record;
mod source;
#[cfg(test)]
mod tests;
mod unpack;
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
use record::{Installed, Record};
use source::Fetched;

pub use cover::get as cover;
pub use pack::{FILE as PACK_FILE, Pack};

/// Where the store keeps a package while it is checked, under the plugins folder; the
/// leading dot keeps it out of the plugins found.
const STAGING: &str = ".staging";

/// One install or removal at a time: two of the same plugin would share its folder.
static BUSY: LazyLock<Mutex<()>> = LazyLock::new(Mutex::default);

/// What the sources offer, with what is installed here, in `order`; only `category`'s
/// when it is given. A source that cannot be read is reported beside the rest.
pub async fn list(
    paths: &Paths,
    mirrors: &[Mirror],
    order: Order,
    category: Option<&str>,
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
                .map(move |entry| listing::listing(paths, record, source, entry))
        })
        .chain(listing::local(paths, &record))
        .collect();
    listing::mark_clashes(&mut listings);
    listings.retain(|listing| category.is_none_or(|category| listing.category == category));
    listing::sort(&mut listings, order);
    Ok(StoreList { listings, failed })
}

/// What the user agreed to install, as they were shown it.
pub struct Agreed<'a> {
    pub source: &'a str,
    pub tier: Tier,
    pub name: &'a str,
    pub version: &'a str,
    pub permissions: &'a [String],
}

/// Installs or updates a plugin in the plugins folder, or a skill, pack or agent in the
/// rules folder, when its listing is still the one
/// the user agreed to: the same source and tier, version and permissions.
/// Answers with what it installed.
pub async fn install(paths: &Paths, mirrors: &[Mirror], agreed: &Agreed<'_>) -> Result<Kind> {
    let _busy = BUSY.lock().await;
    // Every source, since a mirror's tier depends on the official list.
    let (fetched, failed) = source::read(paths, mirrors).await;
    if let Some(failure) = failed.iter().find(|item| item.source == agreed.source) {
        return Err(anyhow!("{}: {}", failure.source, failure.error));
    }
    let (source, entry) = pick(&fetched, agreed)?;
    put(paths, source, entry).await.map(|()| entry.kind)
}

/// The source and entry the user agreed to, refused when anything they were shown has
/// changed since.
pub fn pick<'a>(fetched: &'a [Fetched], agreed: &Agreed<'_>) -> Result<(&'a Fetched, &'a Entry)> {
    let name = agreed.name;
    let source = fetched
        .iter()
        .find(|item| item.name == agreed.source)
        .with_context(|| format!("there is no source named {}", agreed.source))?;
    ensure!(
        source.tier == agreed.tier,
        "{} is {:?} now, not {:?} as you saw it; look at the list again",
        source.name,
        source.tier,
        agreed.tier
    );
    let entry = source
        .index
        .plugins
        .iter()
        .find(|entry| entry.name == name)
        .with_context(|| format!("{} offers no {name}", source.name))?;
    ensure!(
        entry.version == agreed.version && entry.consent() == agreed.permissions,
        "the listing of {name} changed since you saw it; look at the list again"
    );
    Ok((source, entry))
}

/// Updates every plugin a source installed to that source's newer version, unless the
/// update asks for more than the version here, or the source is no longer the one that
/// installed it: the user decides those by hand. Other installs may run between two
/// updates.
pub async fn update_all(paths: &Paths, mirrors: &[Mirror]) -> Result<UpdateReport> {
    let (fetched, failed) = source::read(paths, mirrors).await;
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
    match check_removable(paths, name)? {
        Kind::Plugin => {
            let folder = catalog::folder(paths).join(name);
            fs::remove_dir_all(&folder)
                .with_context(|| format!("cannot remove {}", folder.display()))?;
        }
        _ => content::remove(paths, name)?,
    }
    Record::update(paths, |record| {
        record.installed.remove(name);
        Ok(())
    })
}

/// Downloads, checks and places `entry` from `source`; called under `BUSY`.
async fn put(paths: &Paths, source: &Fetched, entry: &Entry) -> Result<()> {
    let name = &entry.name;
    let download = entry.download_here().map_err(anyhow::Error::msg)?;
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
    let package =
        fetch::fetch_checked(&download.url, download.size, &download.sha256, Via::Package)
            .await
            .with_context(|| format!("the package of {name}"))?;
    let placed = entry.clone();
    let folder = match entry.kind {
        Kind::Plugin => {
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
            tokio::task::spawn_blocking(move || {
                crate::init::create_private_dir(&catalog::folder(&paths))?;
                content::place(&paths, &placed, &package, owned.as_deref())
            })
            .await
            .context("the install stopped")?
            .with_context(|| format!("cannot install {name}"))?
        }
    };
    let mut installed = Installed::new(source, &entry.version, &download.sha256);
    installed.kind = entry.kind;
    installed.folder = folder;
    Record::update(paths, |record| {
        record.installed.insert(name.clone(), installed);
        Ok(())
    })
}

/// Whether `entry` asks for a permission or a model provider the plugin in `folder` does
/// not have.
fn asks_more(folder: &Path, entry: &Entry) -> Result<bool> {
    let manifest = Manifest::load(folder)?;
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
    Ok(())
}
