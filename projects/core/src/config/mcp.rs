//! MCP servers as the config lists them, one YAML document each when edited.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use super::sections::{is_true, yes};

/// An MCP server: a `command` the core starts, or a `url` it reaches.
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpServerConfig {
    Stdio(StdioServerConfig),
    Http(HttpServerConfig),
}

/// A server the core starts as a child process and talks to over stdio. `env` may hold
/// secrets, so it is never logged.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StdioServerConfig {
    pub command: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// A remote server over Streamable HTTP. `headers` go with every request and may hold
/// secrets, such as an authorization, so they are never logged.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpServerConfig {
    pub url: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

impl McpServerConfig {
    pub fn enabled(&self) -> bool {
        match self {
            Self::Stdio(server) => server.enabled,
            Self::Http(server) => server.enabled,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        match self {
            Self::Stdio(server) => server.enabled = enabled,
            Self::Http(server) => server.enabled = enabled,
        }
    }

    /// The command line or the URL, for a client to show.
    pub fn target(&self) -> String {
        match self {
            Self::Stdio(server) => std::iter::once(server.command.as_str())
                .chain(server.args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" "),
            Self::Http(server) => server.url.clone(),
        }
    }
}

/// One MCP server as YAML, `name:` with its fields below, for editing by hand.
pub fn mcp_server_yaml(name: &str, server: &McpServerConfig) -> Result<String> {
    let one = BTreeMap::from([(name, server)]);
    serde_saphyr::to_string(&one).context("cannot encode the MCP server")
}

/// One MCP server from the YAML `mcp_server_yaml` writes. It is checked with the rest
/// of the config when it is saved.
pub fn parse_mcp_server(text: &str) -> Result<(String, McpServerConfig)> {
    // YAML errors can quote values, and headers or env may hold secrets.
    let servers: BTreeMap<String, McpServerConfig> =
        serde_saphyr::from_str(text).map_err(|_| {
            anyhow!("expected `name:` and below it `command:` or `url:` with their fields")
        })?;
    let mut servers = servers.into_iter();
    match (servers.next(), servers.next()) {
        (Some(server), None) => Ok(server),
        _ => Err(anyhow!("write exactly one server")),
    }
}
