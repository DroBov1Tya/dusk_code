//! The Workshop: what its sources offer, as the core read it from their signed lists, and
//! what a client asks of it. A client only shows the list; checking the signatures, the
//! downloads and the files is the core's, and so is the order the list comes in.

use serde::{Deserialize, Serialize};

use super::{Client, Os};

/// The source the core's own key signs; a mirror can never take its name.
pub const OFFICIAL_SOURCE: &str = "official";
/// The source of plugins found here that no source installed.
pub const LOCAL_SOURCE: &str = "local";

/// One thing a source offers, or a plugin found here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    pub name: String,
    pub version: String,
    pub description: String,
    /// All when empty.
    pub platforms: Vec<Os>,
    /// All when empty.
    pub clients: Vec<Client>,
    /// What it may do, one line each, as the user is asked before it is installed.
    pub permissions: Vec<String>,
    /// An `https` page about it.
    pub homepage: Option<String>,
    /// The version here, when this source installed it.
    pub installed: Option<String>,
    /// Why it cannot be installed on this machine, when it cannot.
    pub unfit: Option<String>,
    /// How far it is trusted, from the key that signed its list.
    #[serde(default)]
    pub tier: Tier,
    /// The source's name; with `name`, what the listing is.
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub kind: Kind,
    /// Lower case, `a-z0-9-`; empty when the list gives none.
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// A picture of it, fetched with `StoreRequest::Cover`.
    #[serde(default)]
    pub cover: Option<Cover>,
    /// As the publisher counted them when signing the list.
    #[serde(default)]
    pub stars: u32,
    /// RFC 3339.
    #[serde(default)]
    pub created: Option<String>,
    /// RFC 3339.
    #[serde(default)]
    pub updated: Option<String>,
    /// The source's newer version, when this source installed an older one.
    #[serde(default)]
    pub update: Option<String>,
    /// For a pack: the skills it holds.
    #[serde(default)]
    pub skills: Vec<String>,
    /// Another source offers the same name; a client warns, since a mirror may copy a
    /// name to pass for another source's plugin.
    #[serde(default)]
    pub clash: bool,
}

/// Trust, highest first; the list is sorted in this order.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Signed by the key built into duskcode.
    #[default]
    Official,
    /// From a mirror the official list names with the same address and key.
    Verified,
    /// From a mirror the user added.
    Community,
    /// Found in the plugins folder, not installed by any source: no key vouches for it.
    Local,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Plugin,
    Skill,
    /// A set of skills.
    Pack,
    Agent,
}

/// A picture beside the source's list: a `.png` or `.webp` of at most 512 KiB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cover {
    /// Relative to the list's address.
    pub path: String,
    /// Hex, lower case.
    pub sha256: String,
    pub size: u64,
}

/// The order of the list; the core sorts, so every client shows the same.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    /// Official, Verified, Community, Local, then by name.
    #[default]
    Trust,
    /// Newest `created` first.
    Newest,
    /// Latest `updated` first.
    Updated,
    /// Most stars first.
    Stars,
}

/// The answer to `StoreRequest::List`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreList {
    pub listings: Vec<Listing>,
    /// The sources that could not be read; the others are listed all the same.
    pub failed: Vec<SourceFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFailure {
    pub source: String,
    /// A short reason, without addresses; the core logs the rest.
    pub error: String,
}

/// The answer to `StoreRequest::UpdateAll`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateReport {
    /// `name version`, one each.
    pub updated: Vec<String>,
    /// Updates left for the user, by name, with why.
    pub skipped: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoreRequest {
    /// What the sources offer; answered with `Response::Store`. Only `category`'s, when
    /// it is given.
    List {
        #[serde(default)]
        order: Order,
        #[serde(default)]
        category: Option<String>,
    },
    /// Installs or updates `name` at `version` from `source` and switches it on.
    /// `agreed` is the listing's `permissions` the user was shown; a listing that
    /// changed since is refused.
    /// `tier` is the one the user was shown; the install is refused when the source
    /// earns another one now.
    Install {
        source: String,
        tier: Tier,
        name: String,
        version: String,
        agreed: Vec<String>,
    },
    /// Removes a plugin installed from the store.
    Remove { name: String },
    /// Updates every installed plugin whose update asks for nothing more; answered with
    /// `Response::StoreUpdates`.
    UpdateAll,
    /// The cover of `name` from `source`, as the last list read showed it; answered with
    /// `Response::Cover`.
    Cover { source: String, name: String },
}
