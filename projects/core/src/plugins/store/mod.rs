//! The plugin store: a signed list on GitHub Pages and the packages it names. Installing
//! checks the list's signature, serial and expiry, the package's size and SHA-256,
//! unpacks it under the rules of `unpack` and refuses a package whose manifest differs
//! from the listing the user agreed to. Only a plugin the store installed, by its record
//! in the config folder, is updated or removed by it; a folder the user made is never
//! touched.
//!
//! - `index` — the list and its signature;
//! - `record` — what the store did here;
//! - `unpack` — a package into a folder.

pub mod index;
mod record;
#[cfg(test)]
mod tests;
mod unpack;

use std::{fs, path::Path, sync::LazyLock, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::plugin::Listing;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use super::manifest::Manifest;
use crate::{init::paths::Paths, release::update::VERSION};
use index::{Entry, Index};
use record::{Installed, Record};

const INDEX_URL: &str = "https://drobov1tya.github.io/duskcode-plugins/index.json";
/// Another list, such as a fork's, for trying a store; it is still checked against the
/// key built in, so it cannot bring an unsigned plugin.
const INDEX_ENV: &str = "DUSKCODE_STORE_URL";
/// Where the store keeps a package while it is checked, under the plugins folder; the
/// leading dot keeps it out of the plugins found.
const STAGING: &str = ".staging";
const TIMEOUT: Duration = Duration::from_secs(120);

/// One install or removal at a time: two of the same plugin would share its folder, and
/// the record is read and written whole.
static BUSY: LazyLock<Mutex<()>> = LazyLock::new(Mutex::default);

/// The store's plugins, with what is installed here.
pub async fn list(paths: &Paths) -> Result<Vec<Listing>> {
    let index = fetch_index(paths).await?;
    let record = Record::load(paths)?;
    Ok(index
        .plugins
        .iter()
        .map(|entry| Listing {
            name: entry.name.clone(),
            version: entry.version.clone(),
            description: entry.description.clone(),
            platforms: entry.platforms.clone(),
            clients: entry.clients.clone(),
            permissions: entry.consent(),
            homepage: entry.homepage.clone(),
            installed: record.version_of(paths, &entry.name),
            unfit: entry.download_here().err(),
        })
        .collect())
}

/// Installs or updates `name` at `version` in the plugins folder, when its listing still
/// asks for exactly `agreed`, the lines the user was shown.
pub async fn install(paths: &Paths, name: &str, version: &str, agreed: &[String]) -> Result<()> {
    let _busy = BUSY.lock().await;
    let index = fetch_index(paths).await?;
    let entry = index
        .plugins
        .iter()
        .find(|entry| entry.name == name)
        .with_context(|| format!("the store has no plugin {name}"))?;
    ensure!(
        entry.version == version && entry.consent() == agreed,
        "the store's listing of {name} changed since you saw it; look at the list again"
    );
    let download = entry.download_here().map_err(anyhow::Error::msg)?;
    let package = fetch(&download.url, download.size).await?;
    ensure!(
        package.len() as u64 == download.size,
        "the package of {name} has {} bytes, the list says {}",
        package.len(),
        download.size
    );
    let sha256 = hex(&Sha256::digest(&package));
    ensure!(
        sha256 == download.sha256,
        "the package of {name} is not the one the list names"
    );
    let mut record = Record::load(paths)?;
    let root = super::catalog::folder(paths);
    let owned = record.installed.contains_key(name);
    let entry = entry.clone();
    tokio::task::spawn_blocking(move || place(&root, &entry, &package, owned))
        .await
        .context("the install stopped")?
        .with_context(|| format!("cannot install {name}"))?;
    let installed = Installed {
        version: version.to_owned(),
        sha256,
    };
    record.installed.insert(name.to_owned(), installed);
    record.save(paths)
}

/// Whether the store may remove `name`: it installed it and its folder is there. Asked
/// before anything changes.
pub fn check_removable(paths: &Paths, name: &str) -> Result<()> {
    ensure!(
        crate::init::paths::is_plain_folder_name(name),
        "invalid plugin name"
    );
    ensure!(
        Record::load(paths)?.version_of(paths, name).is_some(),
        "{name} was not installed from the store; remove its folder yourself"
    );
    Ok(())
}

pub async fn remove(paths: &Paths, name: &str) -> Result<()> {
    let _busy = BUSY.lock().await;
    check_removable(paths, name)?;
    let folder = super::catalog::folder(paths).join(name);
    fs::remove_dir_all(&folder).with_context(|| format!("cannot remove {}", folder.display()))?;
    let mut record = Record::load(paths)?;
    record.installed.remove(name);
    record.save(paths)
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

/// The newest signed list, refused when it is older than one accepted here or expired.
async fn fetch_index(paths: &Paths) -> Result<Index> {
    let url = std::env::var(INDEX_ENV).unwrap_or_else(|_| INDEX_URL.to_owned());
    let json = fetch(&url, index::MAX_INDEX_BYTES).await?;
    let signature = fetch(&format!("{url}.sig"), index::MAX_SIGNATURE_BYTES).await?;
    let index = index::verify_builtin(&json, &signature, chrono::Utc::now())?;
    let mut record = Record::load(paths)?;
    ensure!(
        index.serial >= record.serial,
        "the store sent an older list ({}) than one seen here ({}); try again later",
        index.serial,
        record.serial
    );
    if index.serial > record.serial {
        record.serial = index.serial;
        record.save(paths)?;
    }
    Ok(index)
}

/// The body at `url`, over https only, refused past `max` bytes.
async fn fetch(url: &str, max: u64) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .timeout(TIMEOUT)
        .user_agent(format!("duskcode/{VERSION}"))
        .build()?;
    let mut response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("cannot reach {url}"))?
        .error_for_status()
        .with_context(|| format!("the store refused {url}"))?;
    if response.content_length().is_some_and(|length| length > max) {
        bail!("{url} is over {max} bytes");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        body.extend_from_slice(&chunk);
        ensure!(body.len() as u64 <= max, "{url} is over {max} bytes");
    }
    Ok(body)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
