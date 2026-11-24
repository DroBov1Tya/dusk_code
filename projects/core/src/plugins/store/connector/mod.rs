//! A connector listing: an MCP server the store adds to the user's connectors. A remote
//! one is an `https` address; a local one is `npx` or `uvx` starting a package pinned to
//! one version, or a program from a pinned GitHub release (`release`), which the store
//! downloads into a folder of its own (`install`). What it asks of the user are names only,
//! headers or environment variables they fill in after it is added; the list never
//! carries a value. Signing in to a remote one is the MCP sign-in the settings already
//! offer.

use std::collections::BTreeMap;

use anyhow::{Result, bail, ensure};
use dusk_protocol::plugin::{Auth, RunBy, ServerSpec, Tier};
use serde::Deserialize;

use super::{
    index::{Badge, Server},
    record::Installed,
    record::Record,
};
use crate::{
    config::{HttpServerConfig, McpServerConfig, StdioServerConfig},
    init::paths::Paths,
};

pub(super) mod check;
pub mod install;
pub mod release;

use check::{MAX_LINE, check_args, check_command, check_url, secret_name};
pub use check::{host, resolves_publicly};

const MAX_SECRETS: usize = 4;

/// The `server` of a connector entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    /// The `https` address of a remote server.
    pub url: Option<String>,
    /// `npx` or `uvx`, for a local one.
    pub command: Option<String>,
    /// A program from a GitHub release, for a local one.
    pub release: Option<release::Release>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub auth: Auth,
    /// Header names for a remote server, environment variable names for a local one.
    #[serde(default)]
    pub secrets: Vec<String>,
    /// The service's own page about its server.
    pub docs: Option<String>,
    #[serde(default)]
    pub run_by: RunBy,
    /// What the agent reaches through it, one line.
    pub reaches: String,
}

/// The programs a connector may start, with the registry each fetches its package from.
pub(super) const RUNNERS: [(&str, &str, &str); 2] =
    [("npx", "npm", "Node.js"), ("uvx", "PyPI", "uv")];

impl Spec {
    /// The server as a plugin's would be listed, under the connector's name.
    pub fn server(&self, name: &str) -> Server {
        let program = self
            .release
            .as_ref()
            .and_then(|release| release.asset().or(release.assets.values().next()))
            .map(|asset| release::Release::program(asset).to_owned());
        Server {
            name: name.to_owned(),
            command: self.command.clone().or(program),
            args: self.args.clone(),
            url: self.url.clone(),
        }
    }

    /// The package a local one starts and where it comes from: `@scope/x@1.2.3 from npm`.
    pub fn package(&self) -> Option<String> {
        if let Some(release) = &self.release {
            let file = release
                .asset()
                .map_or("no build for here", |asset| &asset.file);
            return Some(format!(
                "{file} from the release {} of github.com/{}",
                release.tag, release.repository
            ));
        }
        let command = self.command.as_deref()?;
        let (_, registry, _) = RUNNERS.iter().find(|(name, ..)| *name == command)?;
        let package = self.args.iter().find(|arg| !arg.starts_with('-'))?;
        Some(format!("{package} from {registry}"))
    }

    /// What a client is shown of it.
    pub fn shown(&self) -> ServerSpec {
        ServerSpec {
            auth: self.auth,
            secrets: self.secrets.clone(),
            docs: self.docs.clone(),
            run_by: self.run_by,
            reaches: self.reaches.clone(),
            package: self.package(),
        }
    }

    /// The MCP server it adds as `name`, as the settings would write it. With secrets to
    /// fill in it is written off, their values empty, so it starts only once the user put
    /// them in. A release's program is started by its absolute path under `paths`.
    pub fn config(&self, paths: &Paths, name: &str) -> McpServerConfig {
        let blank = || -> BTreeMap<String, String> {
            self.secrets
                .iter()
                .map(|name| (name.clone(), String::new()))
                .collect()
        };
        let enabled = self.secrets.is_empty();
        match (&self.url, &self.command) {
            (Some(url), _) => McpServerConfig::Http(HttpServerConfig {
                url: url.clone(),
                enabled,
                headers: blank(),
                icon: None,
            }),
            (None, command) => McpServerConfig::Stdio(Box::new(StdioServerConfig {
                command: match &self.release {
                    Some(release) => release
                        .binary_path(paths, name)
                        .map(|path| path.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    None => command.clone().unwrap_or_default(),
                },
                enabled,
                args: self.args.clone(),
                env: blank(),
                icon: None,
                isolation: None,
            })),
        }
    }

    /// The lines the user agrees to after the one `index::brings` writes for its server.
    /// Below Verified nobody checked the list, so who runs it, what it reaches and its
    /// page are the source's claims; where a sign-in or a key goes is said by its host as
    /// it is written to the config.
    pub fn consent(&self, name: &str, tier: Tier) -> Vec<String> {
        let checked = matches!(tier, Tier::Official | Tier::Verified);
        let claim = |line: String| match checked {
            true => line,
            false => format!("the source says {line}; nobody checked that"),
        };
        let mut lines = Vec::new();
        if let Some(release) = &self.release {
            lines.push(release.consent(name));
        } else if let Some(package) = self.package() {
            lines.push(format!(
                "download {package} the first time {name} starts, and run it as you"
            ));
        }
        lines.push(claim(match self.run_by {
            RunBy::Vendor => format!("{name} is run by the service it reaches"),
            RunBy::Community => format!("{name} is run by somebody other than the service"),
        }));
        lines.push(match checked {
            true => format!("reach {}", self.reaches),
            false => claim(format!("it reaches {}", self.reaches)),
        });
        let goes_to = match (self.url.as_deref().and_then(host), &self.command) {
            (Some(host), _) => format!("sent to {host}"),
            (None, _) => format!("given to the program {}", self.server(name).line()),
        };
        match self.auth {
            Auth::None => {}
            Auth::OAuth => lines.push(format!(
                "ask you to sign in in your browser, on whatever page {0} names; the token you get is sent to {0} with its calls and stays in duskcode's config folder",
                self.url.as_deref().and_then(host).unwrap_or_default()
            )),
            Auth::Key => lines.push(format!(
                "stay off until you put your {} in its settings; they are {goes_to}",
                self.secrets.join(", ")
            )),
        }
        if let Some(docs) = &self.docs {
            lines.push(claim(format!("its page is {docs}")));
        }
        lines
    }

    /// Why it cannot be added here from a source of `tier`, the listing carrying `badge`,
    /// when it cannot. A release's program comes only from the official list's own work.
    pub fn unfit(&self, name: &str, tier: Tier, badge: Option<Badge>) -> Option<String> {
        if let Some(release) = &self.release {
            if tier != Tier::Official || badge != Some(Badge::Official) {
                return Some(
                    "it runs a program from a release, so only the official list may bring it, under its Official badge"
                        .into(),
                );
            }
            return release.unfit(name);
        }
        let command = self.command.as_deref()?;
        if !matches!(tier, Tier::Official | Tier::Verified) {
            return Some(
                "it runs a program, so only an official or verified source may bring it".into(),
            );
        }
        let (_, _, comes_with) = RUNNERS.iter().find(|(name, ..)| *name == command)?;
        (!on_path(command)).then(|| {
            format!("needs {command}, which is not on this computer; it comes with {comes_with}")
        })
    }

    /// The rules of a list's entry.
    pub fn check(&self) -> Result<()> {
        match (&self.url, &self.command, &self.release) {
            (Some(url), None, None) => check_url(url)?,
            (None, Some(command), None) => check_command(command, &self.args)?,
            (None, None, Some(release)) => {
                release.check()?;
                check_args(&self.args)?;
            }
            _ => bail!("a connector names one of an address, a command or a release"),
        }
        let line = self.server("x").line();
        ensure!(
            super::index::is_exact_line(&line, MAX_LINE),
            "the connector is not one plain line of up to {MAX_LINE} characters"
        );
        match self.auth {
            Auth::Key => ensure!(
                (1..=MAX_SECRETS).contains(&self.secrets.len()),
                "a connector with a key names 1 to {MAX_SECRETS} secrets"
            ),
            Auth::OAuth => ensure!(
                self.url.is_some() && self.secrets.is_empty(),
                "only a remote connector signs in, and then without secrets"
            ),
            Auth::None => ensure!(self.secrets.is_empty(), "secrets without a key"),
        }
        let remote = self.url.is_some();
        let mut seen = std::collections::BTreeSet::new();
        for name in &self.secrets {
            ensure!(
                seen.insert(name.to_ascii_lowercase()),
                "the secret {name} is given twice"
            );
            secret_name(name, remote)?;
        }
        if let Some(docs) = &self.docs {
            ensure!(
                docs.starts_with("https://")
                    && !docs.contains(' ')
                    && super::index::is_exact_line(docs, MAX_LINE),
                "the docs are not an https address"
            );
        }
        ensure!(
            super::index::is_line(&self.reaches, MAX_LINE),
            "what it reaches is not one plain line of up to {MAX_LINE} characters"
        );
        Ok(())
    }
}

/// Whether `program` is in a folder of `PATH`, as the core starts MCP servers with it.
fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    // On Windows the core starts `npx` itself, which finds an `.exe` but not `npx.cmd`.
    let name = match cfg!(windows) {
        true => format!("{program}.exe"),
        false => program.to_owned(),
    };
    std::env::split_paths(&path).any(|folder| folder.join(&name).is_file())
}

/// What `server` reaches or runs, exactly, to tell whether a server is still the one the
/// store added: its address, or its command and arguments as a JSON list, so `["a b"]`
/// and `["a", "b"]` differ.
pub fn target(server: &McpServerConfig) -> String {
    match server {
        McpServerConfig::Http(http) => http.url.clone(),
        McpServerConfig::Stdio(stdio) => {
            let line: Vec<&str> = std::iter::once(stdio.command.as_str())
                .chain(stdio.args.iter().map(String::as_str))
                .collect();
            serde_json::to_string(&line).unwrap_or_default()
        }
    }
}

/// Every connector the store added, by name; none when the record cannot be read.
pub fn all_added(paths: &Paths) -> BTreeMap<String, Installed> {
    Record::load(paths)
        .map(|record| record.connectors)
        .unwrap_or_default()
}

/// The connector the store added as `name`, when it added one.
pub fn added(paths: &Paths, name: &str) -> Result<Option<Installed>> {
    Ok(Record::load(paths)?.connectors.get(name).cloned())
}

/// Keeps that the store added `name`.
pub fn remember(paths: &Paths, name: &str, added: Installed) -> Result<()> {
    Record::update(paths, |record| {
        record.connectors.insert(name.to_owned(), added);
        Ok(())
    })
}

/// Whether `new` reaches or runs what `old` did: the same target, or another version of
/// `name`'s release program with the same arguments.
pub fn same_program(
    paths: &Paths,
    name: &str,
    new: &McpServerConfig,
    old: &McpServerConfig,
) -> bool {
    if target(new) == target(old) {
        return true;
    }
    let (McpServerConfig::Stdio(new), McpServerConfig::Stdio(old)) = (new, old) else {
        return false;
    };
    let program = |command: &str| {
        let path = std::path::Path::new(command);
        release::tag_of(paths, name, path)?;
        path.file_name().map(|file| file.to_owned())
    };
    new.args == old.args
        && program(&new.command).is_some()
        && program(&new.command) == program(&old.command)
}

/// The release version the store's record of `name` runs, its tag folder, if any.
pub fn kept_version(paths: &Paths, name: &str) -> Option<String> {
    let added = added(paths, name).ok()??;
    let line: Vec<String> = serde_json::from_str(&added.target).ok()?;
    release::tag_of(paths, name, std::path::Path::new(line.first()?))
}

/// Forgets that the store added `name`.
pub fn forget(paths: &Paths, name: &str) -> Result<()> {
    Record::update(paths, |record| {
        record.connectors.remove(name);
        Ok(())
    })
}

/// The values the user filled in for `old`, the same server added before, carried into
/// `new` for the secrets it still names, with `old`'s switch once all of them are set.
pub fn keep_secrets(new: &mut McpServerConfig, old: &McpServerConfig) {
    let carried = |fresh: &mut BTreeMap<String, String>, had: &BTreeMap<String, String>| {
        for (name, value) in fresh.iter_mut() {
            if let Some(kept) = had.get(name) {
                value.clone_from(kept);
            }
        }
        fresh.values().all(|value| !value.is_empty())
    };
    let filled = match (&mut *new, old) {
        (McpServerConfig::Http(fresh), McpServerConfig::Http(had)) => {
            carried(&mut fresh.headers, &had.headers)
        }
        (McpServerConfig::Stdio(fresh), McpServerConfig::Stdio(had)) => {
            carried(&mut fresh.env, &had.env)
        }
        _ => return,
    };
    new.set_enabled(filled && old.enabled());
}
