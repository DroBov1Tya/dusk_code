//! The plugin store: a signed list on GitHub Pages and the packages it names. Installing
//! checks the list's signature, the package's size and SHA-256, unpacks it under the
//! rules of `unpack` and refuses a package whose manifest asks for other permissions
//! than the list showed the user. Only a plugin installed from the store is updated or
//! removed by it; a folder the user made is never touched.
//!
//! - `index` — the list and its signature;
//! - `unpack` — a package into a folder.

pub mod index;
#[cfg(test)]
mod tests;
mod unpack;

use std::{fs, path::Path, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::plugin::Listing;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::manifest::Manifest;
use crate::{init::paths::Paths, release::update::VERSION};
use index::{Entry, Index};

const INDEX_URL: &str = "https://drobov1tya.github.io/duskcode-plugins/index.json";
/// Another list, such as a fork's, for trying a store; it is still checked against the
/// key built in, so it cannot bring an unsigned plugin.
const INDEX_ENV: &str = "DUSKCODE_STORE_URL";
/// Where the store keeps a package while it is checked, under the plugins folder; the
/// leading dot keeps it out of the plugins found.
const STAGING: &str = ".staging";
/// The file in a plugin's folder that says the store put it there.
const MARKER: &str = ".store.json";
const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Serialize, Deserialize)]
struct Marker {
    version: String,
    sha256: String,
}

/// The store's plugins, with what is installed here.
pub async fn list(paths: &Paths) -> Result<Vec<Listing>> {
    let index = fetch_index().await?;
    let root = super::catalog::folder(paths);
    Ok(index
        .plugins
        .iter()
        .map(|entry| Listing {
            name: entry.name.clone(),
            version: entry.version.clone(),
            description: entry.description.clone(),
            platforms: entry.platforms.clone(),
            clients: entry.clients.clone(),
            permissions: entry.permissions.lines(),
            homepage: entry.homepage.clone(),
            installed: marker(&root.join(&entry.name)).map(|marker| marker.version),
            unfit: entry.download_here().err(),
        })
        .collect())
}

/// Installs or updates `name` at `version` in the plugins folder.
pub async fn install(paths: &Paths, name: &str, version: &str) -> Result<()> {
    let index = fetch_index().await?;
    let entry = index
        .plugins
        .iter()
        .find(|entry| entry.name == name)
        .with_context(|| format!("the store has no plugin {name}"))?;
    ensure!(
        entry.version == version,
        "the store has {name} {} now, not {version}; look at the list again",
        entry.version
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
    let root = super::catalog::folder(paths);
    let entry = entry.clone();
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || place(&root, &entry, &package, &sha256))
        .await
        .context("the install stopped")?
        .with_context(|| format!("cannot install {name}"))
}

/// Removes `name`, when the store installed it.
pub fn remove(paths: &Paths, name: &str) -> Result<()> {
    ensure!(
        crate::init::paths::is_plain_folder_name(name),
        "invalid plugin name"
    );
    let folder = super::catalog::folder(paths).join(name);
    ensure!(
        marker(&folder).is_some(),
        "{name} was not installed from the store; remove its folder yourself"
    );
    fs::remove_dir_all(&folder).with_context(|| format!("cannot remove {}", folder.display()))
}

/// Unpacks the checked package beside the plugins, checks its manifest against the list
/// and swaps it in.
fn place(root: &Path, entry: &Entry, package: &[u8], sha256: &str) -> Result<()> {
    let staging = root.join(STAGING);
    crate::init::create_private_dir(&staging)?;
    let fresh = staging.join(&entry.name);
    let old = staging.join(format!("{}.old", entry.name));
    for leftover in [&fresh, &old] {
        if leftover.exists() {
            fs::remove_dir_all(leftover)?;
        }
    }
    let result = (|| {
        unpack::unpack(package, &fresh)?;
        check_manifest(&fresh, entry)?;
        let record = Marker {
            version: entry.version.clone(),
            sha256: sha256.to_owned(),
        };
        crate::init::files::write_private(&fresh.join(MARKER), &serde_json::to_vec(&record)?)?;
        let target = root.join(&entry.name);
        if target.exists() {
            ensure!(
                marker(&target).is_some(),
                "a plugin named {} is already here and not from the store",
                entry.name
            );
            fs::rename(&target, &old)?;
        }
        fs::rename(&fresh, &target)?;
        Ok(())
    })();
    for leftover in [&fresh, &old] {
        if leftover.exists() {
            let _ = fs::remove_dir_all(leftover);
        }
    }
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
    Ok(())
}

fn marker(folder: &Path) -> Option<Marker> {
    let bytes = crate::init::files::read_regular(&folder.join(MARKER), 4096).ok()?;
    serde_json::from_slice(&bytes).ok()
}

async fn fetch_index() -> Result<Index> {
    let url = std::env::var(INDEX_ENV).unwrap_or_else(|_| INDEX_URL.to_owned());
    let json = fetch(&url, index::MAX_INDEX_BYTES).await?;
    let signature = fetch(&format!("{url}.sig"), index::MAX_SIGNATURE_BYTES).await?;
    index::verify_builtin(&json, &signature)
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
