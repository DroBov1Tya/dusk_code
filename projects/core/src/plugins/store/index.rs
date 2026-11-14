//! The store's list of plugins, `index.json`, and its signature. The list is trusted only
//! when its Ed25519 signature checks against the key built into duskcode; it carries the
//! SHA-256 of every package, so one signature vouches for all the files it names.

use anyhow::{Context, Result, ensure};
use aws_lc_rs::signature::{ED25519, UnparsedPublicKey};
use base64::Engine;
use chrono::{DateTime, Utc};
use dusk_protocol::plugin::{Client, Os, Permissions};
use serde::Deserialize;

/// The public half of the key the store's list is signed with; the private half never
/// leaves the store's maintainers.
const PUBLIC_KEY: &str = "ajB547F8pPEYYXGxEYBRZgpgw+iU57ydMrr6VtaLwBQ=";
/// The list's format; a list in another one is refused rather than half read.
const FORMAT: u32 = 1;
pub const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_SIGNATURE_BYTES: u64 = 1024;
/// The largest package the store may name.
pub const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub format: u32,
    /// Grows with every list signed; a list older than one seen is refused, so an old
    /// signed list cannot bring back an old package or hide an update.
    pub serial: u64,
    /// RFC 3339; past it the list is refused, so a list held back is not trusted forever.
    pub expires: String,
    pub plugins: Vec<Entry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub platforms: Vec<Os>,
    #[serde(default)]
    pub clients: Vec<Client>,
    /// The same as its manifest's; the package is refused when they differ.
    #[serde(default)]
    pub permissions: Permissions,
    pub homepage: Option<String>,
    /// The `base_url` of each model provider it brings, sorted: the core sends chats
    /// there itself, outside the plugin's sandbox, so the user is told.
    #[serde(default)]
    pub providers: Vec<String>,
    pub downloads: Vec<Download>,
}

/// One package of a plugin, for one system and processor or for all.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Download {
    /// Any system when left out.
    pub os: Option<Os>,
    /// `aarch64` or `x86_64`; any processor when left out.
    pub arch: Option<String>,
    pub url: String,
    /// Hex, lower case.
    pub sha256: String,
    pub size: u64,
}

/// The list in `json`, once `signature` (base64 of 64 bytes) is checked against `key`.
pub fn verify(json: &[u8], signature: &[u8], key: &str, now: DateTime<Utc>) -> Result<Index> {
    let engine = base64::engine::general_purpose::STANDARD;
    let key = engine
        .decode(key)
        .context("the store's key is not base64")?;
    let signature = engine
        .decode(String::from_utf8_lossy(signature).trim())
        .context("the store's signature is not base64")?;
    UnparsedPublicKey::new(&ED25519, &key)
        .verify(json, &signature)
        .map_err(|_| anyhow::anyhow!("the store's list is not signed by its key"))?;
    let index: Index = serde_json::from_slice(json).context("the store's list is not valid")?;
    ensure!(
        index.format == FORMAT,
        "the store's list is in format {}; this duskcode reads {FORMAT}, update it",
        index.format
    );
    let expires = DateTime::parse_from_rfc3339(&index.expires)
        .context("the store's list has no valid expiry")?;
    ensure!(
        expires > now,
        "the store's list expired on {}; try again later",
        index.expires
    );
    let mut names = std::collections::BTreeSet::new();
    for entry in &index.plugins {
        ensure!(
            names.insert(&entry.name),
            "the store lists {} twice",
            entry.name
        );
        check(entry).with_context(|| format!("the store's entry {}", entry.name))?;
    }
    Ok(index)
}

/// Checks the list with the key built in.
pub fn verify_builtin(json: &[u8], signature: &[u8], now: DateTime<Utc>) -> Result<Index> {
    verify(json, signature, PUBLIC_KEY, now)
}

fn check(entry: &Entry) -> Result<()> {
    ensure!(
        crate::init::paths::is_plain_folder_name(&entry.name),
        "invalid name"
    );
    ensure!(
        !entry.version.is_empty() && entry.version.len() <= 40,
        "invalid version"
    );
    if let Some(page) = &entry.homepage {
        ensure!(page.starts_with("https://"), "the homepage is not https");
    }
    for url in &entry.providers {
        ensure!(
            (url.starts_with("https://") || url.starts_with("http://"))
                && url.len() <= 200
                && !url.chars().any(char::is_control),
            "a provider's address is not a plain http(s) URL"
        );
    }
    for download in &entry.downloads {
        ensure!(
            download.url.starts_with("https://"),
            "a package is not on https"
        );
        ensure!(
            download.sha256.len() == 64
                && download
                    .sha256
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
            "a package's sha256 is not 64 hex digits"
        );
        ensure!(
            (1..=MAX_PACKAGE_BYTES).contains(&download.size),
            "a package's size is out of range"
        );
    }
    Ok(())
}

impl Entry {
    /// What the user agrees to by installing it, one line each.
    pub fn consent(&self) -> Vec<String> {
        let mut lines = self.permissions.lines();
        for url in &self.providers {
            lines.push(format!("send your chats to {url} when you pick its model"));
        }
        lines
    }

    /// The package for this machine, or why there is none.
    pub fn download_here(&self) -> Result<&Download, String> {
        let os = Os::current();
        let arch = std::env::consts::ARCH;
        if !dusk_protocol::plugin::runs_here(&self.platforms) {
            let names: Vec<&str> = self.platforms.iter().map(|os| os.name()).collect();
            return Err(format!("made for {} only", names.join(", ")));
        }
        self.downloads
            .iter()
            .find(|download| {
                download.os.is_none_or(|wanted| Some(wanted) == os)
                    && download.arch.as_deref().is_none_or(|wanted| wanted == arch)
            })
            .ok_or_else(|| {
                let os = os.map_or(std::env::consts::OS, Os::name);
                format!("no package for {os} on {arch}")
            })
    }
}
