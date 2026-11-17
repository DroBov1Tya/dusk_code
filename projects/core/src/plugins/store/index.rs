//! A source's list, `index.json`, and its signature. A list is trusted only when its
//! Ed25519 signature checks against the key of its source; it carries the SHA-256 of
//! every package and cover, so one signature vouches for all the files it names.
//!
//! Format 1 lists plugins. Format 2 adds what each entry is (`kind`), how it is filed
//! and shown, and, in the official list only, the mirrors it vouches for.

use anyhow::{Context, Result, bail, ensure};
use aws_lc_rs::signature::{ED25519, UnparsedPublicKey};
use base64::Engine;
use chrono::{DateTime, Utc};
use dusk_protocol::plugin::{Client, Kind, Os, Permissions};
use serde::Deserialize;

use crate::config::{Mirror, is_word};

/// The public half of the key the official list is signed with; the private half never
/// leaves the store's maintainers.
pub const PUBLIC_KEY: &str = "ajB547F8pPEYYXGxEYBRZgpgw+iU57ydMrr6VtaLwBQ=";
/// The formats read; a list in another one is refused rather than half read.
const FORMATS: [u32; 2] = [1, 2];
pub const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_SIGNATURE_BYTES: u64 = 1024;
/// The largest package the store may name.
pub const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;
/// The largest cover: a picture for a card, not a gallery.
pub const MAX_COVER_BYTES: u64 = 512 * 1024;
const MAX_TAGS: usize = 8;
const MAX_WORD: usize = 32;
const MAX_COVER_PATH: usize = 200;
const MAX_PACK_SKILLS: usize = 256;

/// The list as written; entries are read one by one, so an entry of a kind this
/// duskcode does not know is left out instead of failing the whole list.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    format: u32,
    serial: u64,
    expires: String,
    plugins: Vec<serde_json::Value>,
    #[serde(default)]
    verified: Vec<Mirror>,
}

#[derive(Debug)]
pub struct Index {
    /// Grows with every list signed; a list older than one seen is refused, so an old
    /// signed list cannot bring back an old package or hide an update.
    pub serial: u64,
    pub plugins: Vec<Entry>,
    /// Mirrors this list vouches for. Only the official list's are believed.
    pub verified: Vec<Mirror>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub kind: Kind,
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
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub cover: Option<Cover>,
    /// Counted by the publisher when signing; duskcode never asks GitHub.
    #[serde(default)]
    pub stars: u32,
    /// RFC 3339.
    pub created: Option<String>,
    /// RFC 3339.
    pub updated: Option<String>,
    /// For a pack: the skills it holds, by name.
    #[serde(default)]
    pub skills: Vec<String>,
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

/// A picture beside the list, by a path under the list's own folder.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cover {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

/// The list in `json`, once `signature` (base64 of 64 bytes) is checked against `key`.
pub fn verify(json: &[u8], signature: &[u8], key: &str, now: DateTime<Utc>) -> Result<Index> {
    let engine = base64::engine::general_purpose::STANDARD;
    let key = engine
        .decode(key)
        .context("the source's key is not base64")?;
    let signature = engine
        .decode(String::from_utf8_lossy(signature).trim())
        .context("the source's signature is not base64")?;
    UnparsedPublicKey::new(&ED25519, &key)
        .verify(json, &signature)
        .map_err(|_| anyhow::anyhow!("the list is not signed by its source's key"))?;
    let raw: Raw = serde_json::from_slice(json).context("the list is not valid")?;
    ensure!(
        FORMATS.contains(&raw.format),
        "the list is in format {}; this duskcode reads 1 and 2, update it",
        raw.format
    );
    let expires =
        DateTime::parse_from_rfc3339(&raw.expires).context("the list has no valid expiry")?;
    ensure!(
        expires > now,
        "the list expired on {}; try again later",
        raw.expires
    );
    ensure!(
        raw.format >= 2 || raw.verified.is_empty(),
        "a format 1 list names verified mirrors"
    );
    for mirror in &raw.verified {
        mirror.check().context("a verified mirror of the list")?;
    }
    let mut names = std::collections::BTreeSet::new();
    let mut plugins = Vec::new();
    for value in raw.plugins {
        let Some(entry) = read_entry(value)? else {
            continue;
        };
        ensure!(
            names.insert(entry.name.clone()),
            "the list names {} twice",
            entry.name
        );
        check(&entry, raw.format).with_context(|| format!("the list's entry {}", entry.name))?;
        plugins.push(entry);
    }
    Ok(Index {
        serial: raw.serial,
        plugins,
        verified: raw.verified,
    })
}

/// An entry, or none when it is of a kind this duskcode does not know: a newer list
/// may offer more than it can install.
fn read_entry(value: serde_json::Value) -> Result<Option<Entry>> {
    if let Some(kind) = value.get("kind") {
        let Some(text) = kind.as_str() else {
            bail!("an entry's kind is not a word");
        };
        if serde_json::from_value::<Kind>(kind.clone()).is_err() {
            let name = value.get("name").and_then(|name| name.as_str());
            tracing::warn!(
                kind = %text.escape_debug(),
                name = %name.unwrap_or("").escape_debug(),
                "store entry of an unknown kind left out"
            );
            return Ok(None);
        }
    }
    serde_json::from_value(value)
        .map(Some)
        .context("an entry of the list is not valid")
}

fn check(entry: &Entry, format: u32) -> Result<()> {
    ensure!(
        crate::init::paths::is_plain_folder_name(&entry.name),
        "invalid name"
    );
    ensure!(
        !entry.version.is_empty() && entry.version.len() <= 40,
        "invalid version"
    );
    ensure!(
        format >= 2 || entry.is_format_1(),
        "fields of format 2 in a format 1 list"
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
            is_sha256(&download.sha256),
            "a package's sha256 is not 64 hex digits"
        );
        ensure!(
            (1..=MAX_PACKAGE_BYTES).contains(&download.size),
            "a package's size is out of range"
        );
    }
    ensure!(
        entry.category.is_empty() || is_word(&entry.category, MAX_WORD),
        "the category is not a word of a-z, 0-9 and -"
    );
    ensure!(
        entry.tags.len() <= MAX_TAGS && entry.tags.iter().all(|tag| is_word(tag, MAX_WORD)),
        "up to {MAX_TAGS} tags of a-z, 0-9 and - are allowed"
    );
    if let Some(cover) = &entry.cover {
        check_cover(cover)?;
    }
    for date in [&entry.created, &entry.updated].into_iter().flatten() {
        DateTime::parse_from_rfc3339(date).context("a date is not RFC 3339")?;
    }
    ensure!(
        entry.kind == Kind::Pack || entry.skills.is_empty(),
        "only a pack names skills"
    );
    ensure!(
        entry.skills.len() <= MAX_PACK_SKILLS
            && entry
                .skills
                .iter()
                .all(|skill| crate::init::paths::is_plain_folder_name(skill)),
        "a pack's skills are not plain names"
    );
    Ok(())
}

/// A relative path to a `.png` or `.webp` under the list's folder, of a checked size.
pub fn check_cover(cover: &Cover) -> Result<()> {
    let path = &cover.path;
    ensure!(
        !path.is_empty()
            && path.len() <= MAX_COVER_PATH
            && path.split('/').all(|part| {
                !part.is_empty()
                    && part != "."
                    && part != ".."
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            }),
        "the cover's path is not a plain relative path"
    );
    ensure!(
        cover_mime(path).is_some(),
        "the cover is not a .png or .webp"
    );
    ensure!(
        is_sha256(&cover.sha256),
        "the cover's sha256 is not 64 hex digits"
    );
    ensure!(
        (1..=MAX_COVER_BYTES).contains(&cover.size),
        "the cover is over {MAX_COVER_BYTES} bytes"
    );
    Ok(())
}

/// The type of a cover, by its extension.
pub fn cover_mime(path: &str) -> Option<&'static str> {
    if path.ends_with(".png") {
        Some("image/png")
    } else if path.ends_with(".webp") {
        Some("image/webp")
    } else {
        None
    }
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
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
        if self.kind != Kind::Plugin {
            return Err("installs in a later duskcode".into());
        }
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

    /// Nothing format 2 added is set, so a format 1 list is read as it always was.
    fn is_format_1(&self) -> bool {
        self.kind == Kind::Plugin
            && self.category.is_empty()
            && self.tags.is_empty()
            && self.cover.is_none()
            && self.stars == 0
            && self.created.is_none()
            && self.updated.is_none()
            && self.skills.is_empty()
    }
}
