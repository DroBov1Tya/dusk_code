//! The Workshop: what its sources offer, as the core read it from their signed lists, and
//! what a client asks of it. A client only shows the list; checking the signatures, the
//! downloads and the files is the core's, and so is the order the list comes in.

use serde::{Deserialize, Serialize};

use super::{Client, Os};

/// The source the core's own key signs; a mirror can never take its name.
pub const OFFICIAL_SOURCE: &str = "official";
/// The source of plugins found here that no source installed.
pub const LOCAL_SOURCE: &str = "local";
/// The categories a listing may be filed under, in the order a client offers them.
pub const CATEGORIES: [&str; 12] = [
    "coding",
    "review",
    "testing",
    "security",
    "research",
    "writing",
    "data",
    "devops",
    "design",
    "productivity",
    "devices",
    "communication",
];

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
    /// What it may do, one line each, as the user is asked before it is installed: first
    /// one line per `programs`, then one per `connectors`, then what else it may reach.
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
    /// Pictures of it at work, in order; each fetched with `StoreRequest::Screenshot`
    /// by its place here.
    #[serde(default)]
    pub screenshots: Vec<ScreenshotRef>,
    /// GitHub stars of the repository it comes from, only for a mirror the user added;
    /// never for an official or a verified listing, whose count was not the item's own.
    #[serde(default)]
    pub repo_stars: Option<RepoStars>,
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
    /// Who made it, one plain line; empty when the list gives none. For a Community
    /// listing it is the mirror's claim, not checked by anyone.
    #[serde(default)]
    pub author: String,
    /// An SPDX expression; empty when the list gives none.
    #[serde(default)]
    pub license: String,
    /// An `https` address of where it comes from: the list's `source`, a name `source`
    /// already has.
    #[serde(default)]
    pub origin: Option<String>,
    /// One to three of `CATEGORIES`; empty when the list gives none.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Another source offers the same name; a client warns, since a mirror may copy a
    /// name to pass for another source's plugin.
    #[serde(default)]
    pub clash: bool,
    /// For a plugin: the MCP servers it brings.
    #[serde(default)]
    pub connectors: Vec<Connector>,
    /// For a plugin: the command lines of its own programs, run on this computer while it
    /// is on.
    #[serde(default)]
    pub programs: Vec<String>,
}

impl Listing {
    /// The lines of `permissions` after those of its programs and connectors: what else
    /// it may reach.
    pub fn reach(&self) -> &[String] {
        let skip = self.programs.len() + self.connectors.len();
        self.permissions.get(skip..).unwrap_or_default()
    }
}

/// An MCP server a plugin brings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connector {
    pub name: String,
    pub runs: Runs,
    pub state: ConnectorState,
}

/// What an MCP server is: a program on this computer, or an address on the network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runs {
    /// The command line, `./` being the plugin's folder.
    Command(String),
    /// An `https` address the core sends the agent's requests to.
    Url(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorState {
    /// The plugin is not here or not on.
    #[default]
    NotAdded,
    /// The plugin is on and its server is among the connectors.
    Added,
    /// A connector the user configured has the name; it is used, never replaced.
    Yours,
}

/// Trust, highest first; the list is sorted in this order.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Signed by the key built into duskcode, and made by duskcode's maintainers.
    #[default]
    Official,
    /// Signed by the key built into duskcode for somebody else's work, or from a mirror
    /// the official list names with the same address and key.
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

/// The star count of a GitHub repository, as the publisher of a list counted it when
/// signing: the repository's, not a rating of the listing, so it is shown with `repo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoStars {
    /// `owner/name` on github.com.
    pub repo: String,
    pub stars: u32,
}

impl RepoStars {
    /// How every client words it: a repository's count, never a rating.
    pub fn label(&self) -> String {
        let noun = if self.stars == 1 { "star" } else { "stars" };
        format!("{} GitHub {noun} of {}", self.stars, self.repo)
    }
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

/// One screenshot of a listing; its bytes come with `StoreRequest::Screenshot`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenshotRef {
    /// Plain text of at most 120 characters, without control characters.
    #[serde(default)]
    pub caption: Option<String>,
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
}

/// The answer to `StoreRequest::List`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreList {
    pub listings: Vec<Listing>,
    /// The sources that could not be read; the others are listed all the same.
    pub failed: Vec<SourceFailure>,
    /// The listings a client shows on its shelves, sorted by the core.
    #[serde(default)]
    pub picks: Picks,
}

/// Indices into `StoreList::listings`, at most `PICKS` each, best first; a listing
/// without the date sorted on is left out. Nothing is picked by stars: a mirror writes
/// its own count, so a shelf of them would rank whoever wrote the biggest number.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Picks {
    pub newest: Vec<usize>,
    pub updated: Vec<usize>,
}

/// How many listings a shelf of `Picks` holds.
pub const PICKS: usize = 12;

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
    /// it is given, and only of `kinds`, all of them when it is empty: each page asks for
    /// its own, so the core decides what a page lists.
    List {
        #[serde(default)]
        order: Order,
        #[serde(default)]
        category: Option<String>,
        #[serde(default)]
        kinds: Vec<Kind>,
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
    /// `Response::Media`.
    Cover { source: String, name: String },
    /// The screenshot at `index` of `name`'s `screenshots`, as the last list read showed
    /// them; answered with `Response::Media`.
    Screenshot {
        source: String,
        name: String,
        index: u32,
    },
    /// The files of `name` from `source`: the copy installed here, or else the package
    /// the last list read named, fetched and checked; answered with
    /// `Response::StoreContents`. Nothing of it is installed, run or read by the model.
    Contents { source: String, name: String },
    /// One text file of the last `Contents` of `name`, by its `path` there; answered with
    /// `Response::StoreFile`.
    File {
        source: String,
        name: String,
        path: String,
    },
    /// Reads the signed list at `url` with the Ed25519 `key` (base64) without saving
    /// anything; answered with `Response::StoreProbe`. `url` is a list's `https` address
    /// or a GitHub repository's, whose list is then on its GitHub Pages site.
    Probe { url: String, key: String },
    /// The repositories the user added, each read now; answered with
    /// `Response::StoreSources`.
    Sources,
    /// Probes `url` with `key` again and, when it passes, saves it as a repository under a
    /// name the core gives it. Its items are Community.
    AddSource { url: String, key: String },
    /// Forgets a repository the user added; what it installed stays.
    RemoveSource { name: String },
}

/// The answer to `StoreRequest::Probe`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    /// The list's address the probe read, after a repository's was turned into it.
    pub url: String,
    pub entries: u32,
    pub plugins: u32,
    pub serial: u64,
    /// RFC 3339.
    pub expires: String,
}

/// A repository the user added, as `StoreRequest::Sources` read it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRow {
    pub name: String,
    pub url: String,
    /// The first 16 hex digits of the SHA-256 of its key.
    pub fingerprint: String,
    /// Community, or Verified when the official list vouches for it.
    pub tier: Tier,
    pub entries: u32,
    /// Why its list was refused; none when it was read.
    pub error: Option<String>,
}

/// The answer to `StoreRequest::Contents`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contents {
    /// Sorted by path.
    pub files: Vec<FileRef>,
    /// The SHA-256 of the package, as the list names it or as it was installed.
    pub package_sha256: String,
    /// The files are the copy installed here, edits included, not the package's.
    pub installed: bool,
}

/// One file of `Contents`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    /// Relative, `/` between names.
    pub path: String,
    pub size: u64,
    /// UTF-8 text a client may ask for with `StoreRequest::File`; anything else, such as
    /// a plugin's program, is shown by its size only.
    pub text: bool,
}

/// The answer to `StoreRequest::File`: plain text, its control and bidi characters
/// escaped, at most `PREVIEW_BYTES` of it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePreview {
    pub text: String,
    /// The file is longer than what `text` holds.
    pub truncated: bool,
}

/// The most of one file `StoreRequest::File` answers with.
pub const PREVIEW_BYTES: usize = 256 * 1024;
