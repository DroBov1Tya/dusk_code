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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// For a skill or pack fetched from its author's repository instead of a package:
    /// `github.com/owner/name at abc1234`.
    #[serde(default)]
    pub upstream: Option<String>,
    /// For an Unverified listing: the host its source's list is read from. Its `author`
    /// and `description` are only that source's claims; `upstream` says where its files
    /// come from.
    #[serde(default)]
    pub from: Option<String>,
    /// For a connector: how its server is reached and what it asks of the user. Its one
    /// entry in `connectors` says what it runs or reaches and whether it is added here.
    #[serde(default)]
    pub server: Option<ServerSpec>,
}

impl Listing {
    /// What every client says above the lines of a connector from a source without a
    /// key, before anything else: nobody vouches for it, where its list comes from, whose
    /// claim its author is, and the exact host its requests, sign-in and keys go to.
    /// Empty for any other listing.
    pub fn unvouched(&self) -> Vec<String> {
        if self.kind != Kind::Connector || self.tier != Tier::Unverified {
            return Vec::new();
        }
        let from = match &self.from {
            Some(host) => format!("{} ({host})", self.source),
            None => self.source.clone(),
        };
        let mut lines = vec![format!(
            "Nobody vouches for {}. It comes from {from}, a source you added without a key: \
             nobody signed its list, and what it says about itself is only its claim.",
            self.name
        )];
        if !self.author.is_empty() {
            lines.push(format!(
                "The source says it is by {}; nothing checks that.",
                self.author
            ));
        }
        for connector in &self.connectors {
            lines.push(match &connector.runs {
                Runs::Url(url) => {
                    let host = url
                        .strip_prefix("https://")
                        .and_then(|rest| rest.split(['/', '?', '#']).next())
                        .unwrap_or(url);
                    format!("Its tool calls, any sign-in and any key you put in go to {host}.")
                }
                Runs::Command(line) => format!("It runs {line} on this computer, as you."),
            });
        }
        lines
    }

    /// The lines of `permissions` after those of its programs and connectors: what else
    /// it may reach.
    pub fn reach(&self) -> &[String] {
        let skip = self.programs.len() + self.connectors.len();
        self.permissions.get(skip..).unwrap_or_default()
    }
}

/// An MCP server a plugin brings.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connector {
    pub name: String,
    pub runs: Runs,
    pub state: ConnectorState,
}

/// What a connector listing needs to become an MCP server here: never a secret's value,
/// only the names the user fills in once it is added.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerSpec {
    pub auth: Auth,
    /// The headers (a remote server) or environment variables (a program) the user fills
    /// in after adding it; the server stays off until they do.
    #[serde(default)]
    pub secrets: Vec<String>,
    /// An `https` page of the service's own about its server.
    #[serde(default)]
    pub docs: Option<String>,
    pub run_by: RunBy,
    /// What the agent can reach through it, one line, as its source says.
    #[serde(default)]
    pub reaches: String,
    /// For a program: the package it fetches and where from, such as
    /// `@playwright/mcp@0.0.86 from npm`.
    #[serde(default)]
    pub package: Option<String>,
}

/// How a connector's server knows the user.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Auth {
    /// It answers anybody.
    #[default]
    None,
    /// The user signs in in their browser once it is added; the token stays in the
    /// config folder.
    #[serde(rename = "oauth")]
    OAuth,
    /// A key or token the user puts in `secrets` themselves.
    Key,
}

/// Who runs a connector's server: the service it reaches, or somebody else.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunBy {
    #[default]
    Vendor,
    Community,
}

/// What an MCP server is: a program on this computer, or an address on the network.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Runs {
    /// The command line, `./` being the plugin's folder.
    Command(String),
    /// An `https` address the core sends the agent's requests to.
    Url(String),
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// From a source the user added without a key: nobody signed its list, so it may
    /// bring only skills, packs, agents and connectors to a public `https` address, and
    /// none of them is updated on its own.
    Unverified,
    /// Found in the plugins folder, not installed by any source: no key vouches for it.
    Local,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Plugin,
    Skill,
    /// A set of skills.
    Pack,
    Agent,
    /// An MCP server to add to the connectors: no files of its own, only where the server
    /// is or the program it starts. A list read by an older duskcode leaves it out.
    Connector,
}

/// The star count of a GitHub repository, as the publisher of a list counted it when
/// signing: the repository's, not a rating of the listing, so it is shown with `repo`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cover {
    /// Relative to the list's address.
    pub path: String,
    /// Hex, lower case.
    pub sha256: String,
    pub size: u64,
}

/// One screenshot of a listing; its bytes come with `StoreRequest::Screenshot`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenshotRef {
    /// Plain text of at most 120 characters, without control characters.
    #[serde(default)]
    pub caption: Option<String>,
}

/// The order of the list; the core sorts, so every client shows the same.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    /// Official, Verified, Community, Unverified, Local, then by name.
    #[default]
    Trust,
    /// Newest `created` first.
    Newest,
    /// Latest `updated` first.
    Updated,
}

/// The answer to `StoreRequest::List`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Picks {
    pub newest: Vec<usize>,
    pub updated: Vec<usize>,
}

/// How many listings a shelf of `Picks` holds.
pub const PICKS: usize = 12;

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFailure {
    pub source: String,
    /// A short reason, without addresses; the core logs the rest.
    pub error: String,
}

/// The answer to `StoreRequest::UpdateAll`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateReport {
    /// `name version`, one each.
    pub updated: Vec<String>,
    /// Updates left for the user, by name, with why.
    pub skipped: Vec<(String, String)>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// Installs or updates `name` at `version` from `source` and switches it on; a
    /// connector's server is added to the connectors as the settings add one.
    /// `agreed` is the listing's `permissions` the user was shown; a listing that
    /// changed since is refused.
    /// `tier` is the one the user was shown; the install is refused when the source
    /// earns another one now.
    /// `sha256` is the `package_sha256` of the `Review` the user was shown; an
    /// Unverified listing is installed only with it, and only while it still holds.
    Install {
        source: String,
        tier: Tier,
        name: String,
        version: String,
        agreed: Vec<String>,
        #[serde(default)]
        sha256: Option<String>,
    },
    /// Removes a plugin, skill, pack, agent or connector installed from the store.
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
    /// What installing `name` from `source` would write, and what it changes in the copy
    /// installed here, fetched and checked like an install without writing anything;
    /// answered with `Response::StoreReview`. Asked before an Unverified listing is
    /// installed or updated.
    Review { source: String, name: String },
    /// Reads the signed list at `url` with the Ed25519 `key` (base64) without saving
    /// anything; answered with `Response::StoreProbe`. `url` is a list's `https` address
    /// or a GitHub repository's, whose list is then on its GitHub Pages site. An empty
    /// `key` reads the list unsigned, as an Unverified source.
    Probe { url: String, key: String },
    /// The repositories the user added, each read now; answered with
    /// `Response::StoreSources`.
    Sources,
    /// Probes `url` with `key` again and, when it passes, saves it as a repository under a
    /// name the core gives it. Its items are Community, or Unverified when `key` is empty.
    AddSource { url: String, key: String },
    /// Forgets a repository the user added; what it installed stays.
    RemoveSource { name: String },
}

/// The answer to `StoreRequest::Probe`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probe {
    /// The list's address the probe read, after a repository's was turned into it.
    pub url: String,
    pub entries: u32,
    pub plugins: u32,
    pub serial: u64,
    /// RFC 3339.
    pub expires: String,
    /// Read without a key: only its skills and packs pinned upstream are kept, and they
    /// are Unverified.
    #[serde(default)]
    pub unverified: bool,
    /// What the core left out of the list, as `SourceRow::note`.
    #[serde(default)]
    pub note: Option<String>,
}

/// A repository the user added, as `StoreRequest::Sources` read it.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRow {
    pub name: String,
    pub url: String,
    /// The first 16 hex digits of the SHA-256 of its key; empty for a source without one.
    pub fingerprint: String,
    /// Community, Verified when the official list vouches for it, or Unverified without
    /// a key.
    pub tier: Tier,
    pub entries: u32,
    /// Why its list was refused; none when it was read.
    pub error: Option<String>,
    /// What the core left out of a list it read, such as the plugins of an Unverified
    /// source.
    #[serde(default)]
    pub note: Option<String>,
}

/// The answer to `StoreRequest::Review`: the files an install would write, by their path
/// in the item's folder, and how they differ from the copy installed here.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    /// What `StoreRequest::Install` names as `sha256` to install exactly these files.
    pub package_sha256: String,
    /// Sorted by path.
    pub files: Vec<FileRef>,
    /// Against the copy installed here; all of `files` when none is.
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
    /// The lines that turn the installed `SKILL.md` or `AGENT.md` into the new one, when
    /// it changed, with its path.
    #[serde(default)]
    pub diff_path: Option<String>,
    #[serde(default)]
    pub diff: Vec<crate::catalog::DiffLine>,
    /// The diff was cut at its caps.
    #[serde(default)]
    pub diff_cut: bool,
    /// Why the install would be refused, when it would.
    #[serde(default)]
    pub refused: Option<String>,
    /// Of `files`, the scripts the agent may run: by their extension or a `#!` line.
    #[serde(default)]
    pub scripts: Vec<String>,
    /// It holds an agent, whose `AGENT.md` becomes a chat's instructions.
    #[serde(default)]
    pub agent: bool,
    /// The tools that agent gets; none here means every tool, as its front matter names
    /// none.
    #[serde(default)]
    pub agent_tools: Option<Vec<String>>,
}

/// The answer to `StoreRequest::Contents`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilePreview {
    pub text: String,
    /// The file is longer than what `text` holds.
    pub truncated: bool,
}

/// The most of one file `StoreRequest::File` answers with.
pub const PREVIEW_BYTES: usize = 256 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unverified_connector_says_who_vouches_and_where_it_goes() {
        let listing = Listing {
            name: "notion-workspace".into(),
            kind: Kind::Connector,
            tier: Tier::Unverified,
            source: "loose".into(),
            from: Some("lists.example.net".into()),
            author: "Notion".into(),
            connectors: vec![Connector {
                name: "notion-workspace".into(),
                runs: Runs::Url("https://mcp.notion-workspace.app/mcp".into()),
                state: ConnectorState::NotAdded,
            }],
            ..Listing::default()
        };
        let lines = listing.unvouched();
        assert!(lines[0].starts_with("Nobody vouches for notion-workspace"));
        assert!(lines[0].contains("loose (lists.example.net)"));
        assert!(lines[1].contains("says it is by Notion; nothing checks that"));
        assert!(lines[2].contains("go to mcp.notion-workspace.app."));
        let vouched = Listing {
            tier: Tier::Verified,
            ..listing.clone()
        };
        assert!(vouched.unvouched().is_empty());
        let skill = Listing {
            kind: Kind::Skill,
            ..listing
        };
        assert!(skill.unvouched().is_empty());
    }
}
