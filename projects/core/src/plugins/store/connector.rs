//! A connector listing: an MCP server the store adds to the user's connectors, with no
//! files of its own. A remote one is an `https` address; a local one is `npx` or `uvx`
//! starting a package pinned to one version. What it asks of the user are names only,
//! headers or environment variables they fill in after it is added; the list never
//! carries a value. Signing in to a remote one is the MCP sign-in the settings already
//! offer.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::plugin::{Auth, RunBy, ServerSpec, Tier};
use serde::Deserialize;

use super::{index::Server, record::Installed, record::Record};
use crate::{
    config::{HttpServerConfig, McpServerConfig, StdioServerConfig},
    init::paths::Paths,
};

/// In characters, as for every line the user is shown.
const MAX_LINE: usize = 200;
const MAX_SECRETS: usize = 4;
const MAX_SECRET_NAME: usize = 64;
const MAX_ARGS: usize = 16;

/// The `server` of a connector entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    /// The `https` address of a remote server.
    pub url: Option<String>,
    /// `npx` or `uvx`, for a local one.
    pub command: Option<String>,
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
const RUNNERS: [(&str, &str, &str); 2] = [("npx", "npm", "Node.js"), ("uvx", "PyPI", "uv")];

impl Spec {
    /// The server as a plugin's would be listed, under the connector's name.
    pub fn server(&self, name: &str) -> Server {
        Server {
            name: name.to_owned(),
            command: self.command.clone(),
            args: self.args.clone(),
            url: self.url.clone(),
        }
    }

    /// The package a local one starts and where it comes from: `@scope/x@1.2.3 from npm`.
    pub fn package(&self) -> Option<String> {
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

    /// The MCP server it adds, as the settings would write it. With secrets to fill in it
    /// is written off, their values empty, so it starts only once the user put them in.
    pub fn config(&self) -> McpServerConfig {
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
                command: command.clone().unwrap_or_default(),
                enabled,
                args: self.args.clone(),
                env: blank(),
                icon: None,
                isolation: None,
            })),
        }
    }

    /// The lines the user agrees to after the one `index::brings` writes for its server.
    pub fn consent(&self, name: &str) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(package) = self.package() {
            lines.push(format!(
                "download {package} the first time {name} starts, and run it as you"
            ));
        }
        lines.push(match self.run_by {
            RunBy::Vendor => format!("{name} is run by the service it reaches"),
            RunBy::Community => format!("{name} is run by somebody other than the service"),
        });
        lines.push(format!("reach {}", self.reaches));
        match self.auth {
            Auth::None => {}
            Auth::OAuth => lines.push(
                "ask you to sign in in your browser; the token stays in duskcode's config folder"
                    .to_owned(),
            ),
            Auth::Key => lines.push(format!(
                "stay off until you put your {} in its settings",
                self.secrets.join(", ")
            )),
        }
        lines
    }

    /// Why it cannot be added here from a source of `tier`, when it cannot.
    pub fn unfit(&self, tier: Tier) -> Option<String> {
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
        match (&self.url, &self.command) {
            (Some(url), None) => check_url(url)?,
            (None, Some(command)) => check_command(command, &self.args)?,
            _ => bail!("a connector names an address or a command, not both"),
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
                seen.insert(name.to_ascii_lowercase()) && secret_name(name, remote),
                "a secret's name is not a plain header or variable name, or is given twice"
            );
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

/// An `https` address on a named public host: never this machine or a private network,
/// which a list has no business pointing the agent's requests at.
fn check_url(url: &str) -> Result<()> {
    ensure!(
        url.starts_with("https://") && !url.contains(' '),
        "a connector's address is not https"
    );
    let parsed = reqwest::Url::parse(url).context("a connector's address is not a URL")?;
    ensure!(
        parsed.username().is_empty() && parsed.password().is_none() && parsed.fragment().is_none(),
        "a connector's address carries credentials or a fragment"
    );
    let host = parsed.host_str().unwrap_or_default();
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let public = match bare.parse::<std::net::IpAddr>() {
        Ok(ip) => crate::net::public::is_public(ip),
        Err(_) => {
            let name = host.trim_end_matches('.').to_ascii_lowercase();
            name.contains('.')
                && ![".localhost", ".local", ".internal", ".test", ".home.arpa"]
                    .iter()
                    .any(|end| name.ends_with(end))
        }
    };
    ensure!(public, "a connector's address is not on a public host");
    Ok(())
}

/// `npx` or `uvx`, its own flags, then a package pinned to one exact version, then the
/// package's arguments.
fn check_command(command: &str, args: &[String]) -> Result<()> {
    ensure!(
        RUNNERS.iter().any(|(name, ..)| *name == command),
        "a connector's command is npx or uvx"
    );
    ensure!(args.len() <= MAX_ARGS, "up to {MAX_ARGS} arguments");
    let flags = match command {
        "npx" => ["-y", "--yes"].as_slice(),
        _ => [].as_slice(),
    };
    let start = args
        .iter()
        .position(|arg| !arg.starts_with('-'))
        .context("a connector names no package")?;
    ensure!(
        args[..start]
            .iter()
            .all(|arg| flags.contains(&arg.as_str())),
        "only -y may come before the package"
    );
    ensure!(
        command != "npx" || start > 0,
        "npx is told -y, so it never waits for a yes nobody sees"
    );
    let package = &args[start];
    let (name, version) = match command {
        "npx" => package
            .rsplit_once('@')
            .filter(|(name, _)| !name.is_empty())
            .context("the package is not pinned: name@version")?,
        _ => package
            .split_once("==")
            .context("the package is not pinned: name==version")?,
    };
    ensure!(
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "@/._-".contains(c)),
        "the package's name is not plain"
    );
    ensure!(
        version.starts_with(|c: char| c.is_ascii_digit())
            && version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c)),
        "the package is not pinned to one exact version"
    );
    ensure!(
        args.iter()
            .all(|arg| super::index::is_exact_line(arg, MAX_LINE)),
        "an argument is not one plain line"
    );
    Ok(())
}

/// A header name for a remote server; a variable name for a local one.
fn secret_name(name: &str, header: bool) -> bool {
    let fits = !name.is_empty() && name.len() <= MAX_SECRET_NAME;
    fits && match header {
        true => name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        false => {
            !name.starts_with(|c: char| c.is_ascii_digit())
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        }
    }
}

/// Whether `program` is in a folder of `PATH`, as the core starts MCP servers with it.
fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let names: &[String] = &if cfg!(windows) {
        vec![format!("{program}.cmd"), format!("{program}.exe")]
    } else {
        vec![program.to_owned()]
    };
    std::env::split_paths(&path).any(|folder| names.iter().any(|name| folder.join(name).is_file()))
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
