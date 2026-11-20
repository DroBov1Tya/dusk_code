//! What the store did on this machine, in `store.json` of the config folder, where no
//! plugin's program can write: the newest list it accepted from each source, so an older
//! signed list cannot roll it back, and the plugins it installed with their source, so it
//! never takes a folder the user made, and updates a plugin only from where it came.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use base64::Engine;
use dusk_protocol::{
    plugin::{Kind, OFFICIAL_SOURCE, Tier},
    text::sha256_hex,
};

use super::{index::PUBLIC_KEY, source::Fetched};
use crate::init::paths::Paths;

const FILE: &str = "store.json";
const MAX_BYTES: u64 = 1024 * 1024;

/// Changes are read, made and written under it, so a list read beside an install does
/// not write back a record that lacks the install.
static WRITING: Mutex<()> = Mutex::new(());

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Record {
    /// The highest `serial` of an official list accepted here.
    pub serial: u64,
    /// The same for each mirror, by the address of its list.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mirrors: BTreeMap<String, u64>,
    /// The names the official list offered when it was last accepted, so they stay
    /// taken for an Unverified source while that list cannot be read.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub official_names: std::collections::BTreeSet<String>,
    pub installed: BTreeMap<String, Installed>,
    /// The connectors it added, by the name of their MCP server in the config; apart from
    /// `installed`, so a duskcode that knows no connectors reads the record as before.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub connectors: BTreeMap<String, Installed>,
    /// Folders of items removed from a source other than the official one that still
    /// hold files the user edited, by their folder under the rules folder: what is left
    /// stays that source's, never the user's own, while the folder is there.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub left: BTreeMap<String, Left>,
}

/// Where the files left in a removed item's folder came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Left {
    pub source: String,
    /// Nothing signed the list it came from.
    pub unverified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    pub version: String,
    pub sha256: String,
    /// Records written before mirrors came from the official store.
    #[serde(default = "official")]
    pub source: String,
    /// The address of the source's list; empty for the official one, which may move.
    #[serde(default)]
    pub url: String,
    /// The fingerprint of the key that signed the list it came from; empty when nothing
    /// signed it, from an Unverified source.
    #[serde(default = "official_key")]
    pub key: String,
    /// Records written before skills and packs installed are plugins.
    #[serde(default, skip_serializing_if = "is_plugin")]
    pub kind: Kind,
    /// Where a skill, pack or agent went, under the rules folder, such as
    /// `skills/review`; empty for a plugin, whose folder is in the plugins folder.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
    /// Who made it, its license and where it came from, as the list said at install.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub origin: String,
    /// For a connector: the command line or address it added, so a server the user
    /// changed since is theirs, not the store's to remove.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    /// For a connector: the tier its listing earned when it was added, shown beside its
    /// server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<Tier>,
}

fn is_plugin(kind: &Kind) -> bool {
    *kind == Kind::Plugin
}

fn official() -> String {
    OFFICIAL_SOURCE.to_owned()
}

fn official_key() -> String {
    fingerprint(PUBLIC_KEY)
}

/// The SHA-256 of a base64 public key's bytes, in hex; of the text when it is not base64.
pub fn fingerprint(key: &str) -> String {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(key)
        .unwrap_or_else(|_| key.as_bytes().to_vec());
    sha256_hex(&bytes)
}

impl Installed {
    /// What `source` installs now, at this version.
    pub fn new(source: &Fetched, version: &str, sha256: &str) -> Self {
        Self {
            version: version.to_owned(),
            sha256: sha256.to_owned(),
            source: source.name.clone(),
            url: match source.tier {
                Tier::Official => String::new(),
                _ => source.url.clone(),
            },
            key: key_of(source),
            kind: Kind::Plugin,
            folder: String::new(),
            author: String::new(),
            license: String::new(),
            origin: String::new(),
            target: String::new(),
            tier: None,
        }
    }

    /// From a source without a key: nothing vouches for it.
    pub fn unverified(&self) -> bool {
        self.key.is_empty()
    }

    /// Its folder: in the plugins folder for a plugin, else where it was put.
    pub fn path(&self, paths: &Paths, name: &str) -> PathBuf {
        match self.kind {
            Kind::Plugin => super::super::catalog::folder(paths).join(name),
            _ => paths.rules_root().join(&self.folder),
        }
    }

    /// Whether `source` is the one that installed it: the same name and key, and for a
    /// mirror the same address. A mirror renamed onto another's name, or one that
    /// changed its key or address, is not.
    pub fn is_from(&self, source: &Fetched) -> bool {
        self.source == source.name
            && self.key == key_of(source)
            && (source.tier == Tier::Official || self.url == source.url)
    }
}

impl Record {
    /// The record, or an empty one when there is none yet. A damaged record reads as an
    /// error rather than as empty, which would forget the serials.
    pub fn load(paths: &Paths) -> Result<Self> {
        let file = file(paths);
        if !file.exists() {
            return Ok(Self::default());
        }
        let bytes = crate::init::files::read_regular(&file, MAX_BYTES)?;
        serde_json::from_slice(&bytes).context("the store's record is damaged")
    }

    /// Loads, changes and saves the record as one step.
    pub fn update<T>(paths: &Paths, change: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let _writing = WRITING.lock();
        let mut record = Self::load(paths)?;
        let result = change(&mut record)?;
        crate::init::files::write_private(&file(paths), &serde_json::to_vec_pretty(&record)?)?;
        Ok(result)
    }

    /// What the store installed as `name`, when its folder is still there.
    pub fn present(&self, paths: &Paths, name: &str) -> Option<&Installed> {
        self.installed
            .get(name)
            .filter(|item| item.path(paths, name).is_dir())
    }

    /// How far `name` is vouched for: a folder the store did not install is `Local`; one
    /// signed by the key built in is `Official`; one nothing signed is `Unverified`; any
    /// other source is `Community`.
    pub fn tier(&self, paths: &Paths, name: &str) -> Tier {
        match self.present(paths, name) {
            None => Tier::Local,
            Some(item) if item.key == official_key() => Tier::Official,
            Some(item) if item.unverified() => Tier::Unverified,
            Some(_) => Tier::Community,
        }
    }

    /// What `source` installed as `name`, when its folder is still there.
    pub fn installed_from(
        &self,
        paths: &Paths,
        source: &Fetched,
        name: &str,
    ) -> Option<&Installed> {
        self.present(paths, name)
            .filter(|item| item.is_from(source))
    }
}

/// The fingerprint of the key that checked `source`'s list; empty when none did.
fn key_of(source: &Fetched) -> String {
    match source.key.is_empty() {
        true => String::new(),
        false => fingerprint(&source.key),
    }
}

fn file(paths: &Paths) -> PathBuf {
    paths.config_root().join(FILE)
}
