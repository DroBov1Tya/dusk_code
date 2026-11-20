//! A source's list, `index.json`, and its signature. A list is trusted only when its
//! Ed25519 signature checks against the key of its source; it carries the SHA-256 of
//! every package, cover and screenshot, so one signature vouches for all the files it
//! names.
//!
//! Format 1 lists plugins. Format 2 adds what each entry is (`kind`), how it is filed
//! and shown, and, in the official list only, the mirrors it vouches for. Format 3 adds
//! who made each entry: `author`, `license`, `source`, `categories` and `badge`, all
//! required; and for a plugin what it runs and adds, `mcp`, `programs` and `skills`,
//! which its manifest must say exactly.
//!
//! A connector (`kind: connector`, formats 2 and 3) has no package: its `server` says
//! where the MCP server is or which pinned package starts it. A duskcode before it leaves
//! such an entry out, as any kind it does not know.

use anyhow::{Context, Result, bail, ensure};
use aws_lc_rs::signature::{ED25519, UnparsedPublicKey};
use base64::Engine;
use chrono::{DateTime, Utc};
use dusk_protocol::plugin::{CATEGORIES, Client, Kind, Os, Permissions};
use serde::Deserialize;

use crate::config::{Mirror, is_word};

/// The public half of the key the official list is signed with; the private half never
/// leaves the store's maintainers.
pub const PUBLIC_KEY: &str = "ajB547F8pPEYYXGxEYBRZgpgw+iU57ydMrr6VtaLwBQ=";
/// The formats read; a list in another one is refused rather than half read.
const FORMATS: [u32; 3] = [1, 2, 3];
pub const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_SIGNATURE_BYTES: u64 = 1024;
/// More than a list is ever expected to hold; a list past it is refused whole.
pub const MAX_ENTRIES: usize = 2000;
/// The largest package the store may name.
pub const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;
/// The largest cover: a picture for a card, not a gallery.
pub const MAX_COVER_BYTES: u64 = 512 * 1024;
/// The largest screenshot: a 1600x1000 shot of an interface is 100-600 KiB as WebP or
/// PNG, so 1 MiB leaves room and keeps eight of them under 8 MiB per item.
pub const MAX_SCREENSHOT_BYTES: u64 = 1024 * 1024;
pub const MAX_SCREENSHOTS: usize = 8;
/// In characters; a line under a picture, not a description.
pub const MAX_CAPTION: usize = 120;
const MAX_TAGS: usize = 8;
const MAX_WORD: usize = 32;
const MAX_PICTURE_PATH: usize = 200;
const MAX_PACK_SKILLS: usize = 256;
/// In characters, for `author` and `license`; `source` is an address of `MAX_URL` bytes.
const MAX_CREDIT: usize = 64;
const MAX_URL: usize = 200;
const MAX_CATEGORIES: usize = 3;
const MAX_SERVERS: usize = 16;
const MAX_PROGRAMS: usize = 8;
/// In characters, for a command line or an address shown to the user.
const MAX_LINE: usize = 200;
/// The system and processor of a package of plain files, which runs nowhere.
pub const ANY: &str = "any";
const SYSTEMS: [&str; 4] = [ANY, "macos", "linux", "windows"];

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
    /// RFC 3339, in the future.
    pub expires: String,
    pub plugins: Vec<Entry>,
    /// Mirrors this list vouches for. Only the official list's are believed.
    pub verified: Vec<Mirror>,
    /// What was left out of `plugins` because its source may not bring it, one line.
    pub left_out: Option<String>,
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
    /// Empty only when `upstream` names the files instead.
    #[serde(default)]
    pub downloads: Vec<Download>,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub cover: Option<Cover>,
    /// Pictures of it at work, in the order shown.
    #[serde(default)]
    pub screenshots: Vec<Screenshot>,
    /// GitHub stars of the repository `homepage` names, counted by the publisher when
    /// signing; duskcode never asks GitHub. Shown only for a mirror the user added.
    #[serde(default)]
    pub stars: u32,
    /// RFC 3339.
    pub created: Option<String>,
    /// RFC 3339.
    pub updated: Option<String>,
    /// For a pack: the skills it holds, by name.
    #[serde(default)]
    pub skills: Vec<String>,
    /// Format 3: who made it, one plain line.
    pub author: Option<String>,
    /// Format 3: an SPDX expression.
    pub license: Option<String>,
    /// Format 3: an `https` address of where it comes from.
    pub source: Option<String>,
    /// Format 3: one to three of `CATEGORIES`.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Format 3: whose work it is, as the publisher says. Read only from the official
    /// list, and only to lower its tier.
    pub badge: Option<Badge>,
    /// Format 3, a plugin: the MCP servers it brings, by name.
    #[serde(default)]
    pub mcp: Vec<Server>,
    /// Format 3, a plugin: the command lines of its own programs (its platform, panel and
    /// hooks), sorted.
    #[serde(default)]
    pub programs: Vec<String>,
    /// Format 3, a skill, pack or agent: its files at a pinned commit of the author's
    /// repository, instead of `downloads`.
    pub upstream: Option<super::upstream::Upstream>,
    /// A connector: its MCP server, instead of `downloads`.
    pub server: Option<super::connector::Spec>,
    /// The list's format, which says what its manifest is checked against.
    #[serde(skip)]
    pub format: u32,
}

/// An MCP server a plugin brings: a program it starts, or an address the core sends the
/// agent's requests to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    pub name: String,
    /// The program, `./` being the plugin's folder.
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub url: Option<String>,
}

impl Server {
    /// What it runs or reaches, as one line.
    pub fn line(&self) -> String {
        match (&self.command, &self.url) {
            (Some(command), _) => std::iter::once(command)
                .chain(&self.args)
                .cloned()
                .collect::<Vec<_>>()
                .join(" "),
            (None, Some(url)) => url.clone(),
            (None, None) => String::new(),
        }
    }
}

/// Official for the work of duskcode's maintainers, Verified for anybody else's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Badge {
    Official,
    Verified,
}

/// One package of a plugin, for one system and processor or for all.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Download {
    /// A system's name, or `any` for the one package of a skill, pack or agent; any
    /// system when left out.
    pub os: Option<String>,
    /// `aarch64`, `x86_64` or `any`; any processor when left out.
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

/// A picture of the item at work, beside the list like a cover, with a line under it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Screenshot {
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub caption: Option<String>,
}

impl Screenshot {
    /// The file it names, checked and fetched like a cover.
    pub fn picture(&self) -> Cover {
        Cover {
            path: self.path.clone(),
            sha256: self.sha256.clone(),
            size: self.size,
        }
    }
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
    parse(json, now)
}

/// The list in `json` with every check but the signature's, for a source without a key:
/// what it offers is Unverified, and `guard` keeps only what such a source may bring.
pub fn parse(json: &[u8], now: DateTime<Utc>) -> Result<Index> {
    let raw: Raw = serde_json::from_slice(json).context("the list is not valid")?;
    ensure!(
        FORMATS.contains(&raw.format),
        "the list is in format {}; this duskcode reads 1 to 3, update it",
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
        raw.plugins.len() <= MAX_ENTRIES,
        "the list names {} entries; at most {MAX_ENTRIES} are read",
        raw.plugins.len()
    );
    ensure!(
        raw.format >= 2 || raw.verified.is_empty(),
        "a format 1 list names verified mirrors"
    );
    for mirror in &raw.verified {
        mirror
            .check_signed()
            .context("a verified mirror of the list")?;
    }
    let mut names = std::collections::BTreeSet::new();
    let mut plugins = Vec::new();
    for value in raw.plugins {
        let Some(mut entry) = read_entry(value)? else {
            continue;
        };
        entry.format = raw.format;
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
        expires: raw.expires,
        plugins,
        verified: raw.verified,
        left_out: None,
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
    if format >= 3 {
        check_credit(entry)?;
    } else {
        ensure!(
            entry.is_format_2(),
            "fields of format 3 in a format {format} list"
        );
    }
    if entry.kind == Kind::Connector {
        return check_connector(entry);
    }
    ensure!(entry.server.is_none(), "only a connector names a server");
    match &entry.upstream {
        Some(upstream) => {
            ensure!(
                entry.kind != Kind::Plugin && entry.downloads.is_empty(),
                "only a skill, pack or agent comes from upstream, and then without packages"
            );
            upstream.check()?;
        }
        None => ensure!(!entry.downloads.is_empty(), "no package"),
    }
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
            download
                .os
                .as_deref()
                .is_none_or(|os| SYSTEMS.contains(&os)),
            "a package names an unknown system"
        );
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
    ensure!(
        entry.screenshots.len() <= MAX_SCREENSHOTS,
        "up to {MAX_SCREENSHOTS} screenshots are allowed"
    );
    for screenshot in &entry.screenshots {
        check_screenshot(screenshot)?;
    }
    for date in [&entry.created, &entry.updated].into_iter().flatten() {
        DateTime::parse_from_rfc3339(date).context("a date is not RFC 3339")?;
    }
    ensure!(
        entry.kind == Kind::Pack || entry.kind == Kind::Plugin || entry.skills.is_empty(),
        "only a pack or a plugin names skills"
    );
    ensure!(
        entry.kind == Kind::Plugin || (entry.mcp.is_empty() && entry.programs.is_empty()),
        "only a plugin brings MCP servers or programs"
    );
    check_brings(entry)?;
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

/// A connector: its server and how it is shown, nothing to download or unpack, and none
/// of what a package brings. Its name is its MCP server's.
fn check_connector(entry: &Entry) -> Result<()> {
    let server = entry
        .server
        .as_ref()
        .context("a connector names no server")?;
    ensure!(
        is_word(&entry.name, MAX_WORD),
        "a connector's name is a word of a-z, 0-9 and -"
    );
    ensure!(
        entry.downloads.is_empty()
            && entry.upstream.is_none()
            && entry.mcp.is_empty()
            && entry.programs.is_empty()
            && entry.skills.is_empty()
            && entry.providers.is_empty()
            && entry.permissions == Permissions::default(),
        "a connector has no package, programs, skills, providers or permissions"
    );
    server.check()?;
    if let Some(page) = &entry.homepage {
        ensure!(page.starts_with("https://"), "the homepage is not https");
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
    ensure!(
        entry.screenshots.len() <= MAX_SCREENSHOTS,
        "up to {MAX_SCREENSHOTS} screenshots are allowed"
    );
    for screenshot in &entry.screenshots {
        check_screenshot(screenshot)?;
    }
    for date in [&entry.created, &entry.updated].into_iter().flatten() {
        DateTime::parse_from_rfc3339(date).context("a date is not RFC 3339")?;
    }
    Ok(())
}

/// A plugin's servers and programs: a few, each one plain line, named once.
fn check_brings(entry: &Entry) -> Result<()> {
    ensure!(
        entry.mcp.len() <= MAX_SERVERS,
        "up to {MAX_SERVERS} MCP servers are allowed"
    );
    let mut names = std::collections::BTreeSet::new();
    for server in &entry.mcp {
        ensure!(
            is_word(&server.name, MAX_WORD) && names.insert(&server.name),
            "an MCP server's name is not a word of a-z, 0-9 and -, or is given twice"
        );
        match (&server.command, &server.url) {
            (Some(_), None) => {}
            (None, Some(url)) => ensure!(
                url.starts_with("https://") && !url.contains(' '),
                "the MCP server {} is not at an https address",
                server.name
            ),
            _ => bail!(
                "the MCP server {} names a command or an address, not both",
                server.name
            ),
        }
        ensure!(
            is_line(&server.line(), MAX_LINE),
            "the MCP server {} is not one plain line of up to {MAX_LINE} characters",
            server.name
        );
    }
    ensure!(
        entry.programs.len() <= MAX_PROGRAMS
            && entry.programs.iter().all(|line| is_line(line, MAX_LINE))
            && entry.programs.windows(2).all(|pair| pair[0] < pair[1]),
        "programs are up to {MAX_PROGRAMS} plain lines, sorted, each once"
    );
    Ok(())
}

/// Format 3's fields: all given, each one plain line, the categories from the fixed list.
fn check_credit(entry: &Entry) -> Result<()> {
    let author = entry.author.as_deref().context("no author")?;
    ensure!(
        is_line(author, MAX_CREDIT),
        "the author is not one plain line of up to {MAX_CREDIT} characters"
    );
    let license = entry.license.as_deref().context("no license")?;
    ensure!(
        is_line(license, MAX_CREDIT),
        "the license is not one plain line of up to {MAX_CREDIT} characters"
    );
    let source = entry.source.as_deref().context("no source")?;
    ensure!(
        source.starts_with("https://")
            && source.len() <= MAX_URL
            && is_line(source, MAX_URL)
            && !source.contains(' '),
        "the source is not an https address"
    );
    let categories = &entry.categories;
    ensure!(
        (1..=MAX_CATEGORIES).contains(&categories.len())
            && categories
                .iter()
                .all(|item| CATEGORIES.contains(&item.as_str()))
            && categories
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == categories.len(),
        "the categories are not 1 to {MAX_CATEGORIES} different ones of the fixed list"
    );
    ensure!(entry.badge.is_some(), "no badge");
    Ok(())
}

/// A relative path to a `.png` or `.webp` under the list's folder, of a checked size.
pub fn check_cover(cover: &Cover) -> Result<()> {
    check_picture(cover, MAX_COVER_BYTES).context("the cover")
}

/// A picture like a cover, up to `MAX_SCREENSHOT_BYTES`, and a caption a client can
/// show as one plain line.
pub fn check_screenshot(screenshot: &Screenshot) -> Result<()> {
    check_picture(&screenshot.picture(), MAX_SCREENSHOT_BYTES).context("a screenshot")?;
    if let Some(caption) = &screenshot.caption {
        ensure!(
            is_line(caption, MAX_CAPTION),
            "a screenshot's caption is not one plain line of up to {MAX_CAPTION} characters"
        );
    }
    Ok(())
}

fn check_picture(picture: &Cover, max: u64) -> Result<()> {
    let path = &picture.path;
    ensure!(
        !path.is_empty()
            && path.len() <= MAX_PICTURE_PATH
            && path.split('/').all(|part| {
                !part.is_empty()
                    && part != "."
                    && part != ".."
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            }),
        "the path is not a plain relative path"
    );
    ensure!(cover_mime(path).is_some(), "it is not a .png or .webp");
    ensure!(
        is_sha256(&picture.sha256),
        "the sha256 is not 64 hex digits"
    );
    ensure!(
        (1..=max).contains(&picture.size),
        "the size is not 1 to {max} bytes"
    );
    Ok(())
}

/// Not empty, not padded, at most `max` characters, and none that is a control or one
/// that turns the text around (bidi overrides and isolates), which could make a caption
/// or a name read as something else.
pub fn is_line(text: &str, max: usize) -> bool {
    let bidi = |c: char| matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    !text.trim().is_empty()
        && text.trim() == text
        && text.chars().count() <= max
        && !text.chars().any(|c| c.is_control() || bidi(c))
}

/// The type of a cover or screenshot, by its extension.
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

/// One line per program, then one per MCP server, as the user is asked.
pub fn brings(programs: &[String], servers: &[Server]) -> Vec<String> {
    let mut lines: Vec<String> = programs
        .iter()
        .map(|program| format!("run the program {program} on this computer while it is on"))
        .collect();
    lines.extend(servers.iter().map(|server| match &server.url {
        Some(url) => format!(
            "send the agent's requests for the tools of {} to {url}",
            server.name
        ),
        None => format!(
            "start the MCP server {} ({}) on this computer; the agent may use its tools",
            server.name,
            server.line()
        ),
    }));
    lines
}

impl Entry {
    /// What the user agrees to by installing it, one line each: first one per program,
    /// then one per MCP server, in their order, as `Listing::permissions` says.
    pub fn consent(&self) -> Vec<String> {
        if let Some(server) = self
            .server
            .as_ref()
            .filter(|_| self.kind == Kind::Connector)
        {
            let mut lines = brings(&[], &[server.server(&self.name)]);
            lines.extend(server.consent(&self.name));
            return lines;
        }
        let mut lines = brings(&self.programs, &self.mcp);
        lines.extend(self.permissions.lines());
        for url in &self.providers {
            lines.push(format!("send your chats to {url} when you pick its model"));
        }
        lines
    }

    /// Why it cannot be installed here, when it cannot: an item pinned upstream whose
    /// files are too many bytes, or one with no package for this machine.
    pub fn unfit(&self) -> Option<String> {
        match &self.upstream {
            Some(upstream) => upstream.fits().err().map(|error| error.to_string()),
            None => self.download_here().err(),
        }
    }

    /// The package for this machine, or why there is none.
    /// A skill, pack or agent is plain files, so its one package is for `any` system
    /// and processor; a plugin runs programs, so a package of it for `any` is refused.
    pub fn download_here(&self) -> Result<&Download, String> {
        let any = |download: &Download| {
            download.os.as_deref() == Some(ANY) && download.arch.as_deref() == Some(ANY)
        };
        if self.kind != Kind::Plugin {
            return self
                .downloads
                .iter()
                .find(|download| any(download))
                .ok_or_else(|| "no package for any system".into());
        }
        if self.downloads.iter().any(|download| {
            download.os.as_deref() == Some(ANY) || download.arch.as_deref() == Some(ANY)
        }) {
            return Err("a plugin runs programs, so it cannot be for any system".into());
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
                download
                    .os
                    .as_deref()
                    .is_none_or(|wanted| Some(wanted) == os.map(Os::name))
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
            && self.screenshots.is_empty()
            && self.stars == 0
            && self.created.is_none()
            && self.updated.is_none()
            && self.skills.is_empty()
    }

    /// Nothing format 3 added is set.
    fn is_format_2(&self) -> bool {
        self.mcp.is_empty()
            && self.upstream.is_none()
            && self.programs.is_empty()
            && (self.kind != Kind::Plugin || self.skills.is_empty())
            && self.author.is_none()
            && self.license.is_none()
            && self.source.is_none()
            && self.categories.is_empty()
            && self.badge.is_none()
    }
}
