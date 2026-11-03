//! MCP servers the config lists: local ones the core starts as child processes, remote
//! ones it reaches over HTTP. Each is started in the background when the core starts;
//! its tools join every thread started after it is ready. A server that fails stays
//! failed, and says why, until it is restarted from a client or the core starts again.

mod client;
mod oauth;

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use anyhow::{Context, Result};
use reqwest::Url;

use tokio::sync::watch;
use tracing::{info, warn};

pub use client::{McpClient, RemoteTool, check_headers, check_url};
pub use oauth::TokenFile;

use crate::{
    config::{Config, McpServerConfig},
    tools::{ToolKind, mcp::McpTool},
    util::sync::{read, write},
};

/// How a server stands, for clients to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpState {
    /// Switched off in the config.
    Off,
    Starting,
    /// The names of its tools, as the server gave them.
    Ready(Vec<String>),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpStatus {
    pub name: String,
    pub state: McpState,
}

/// A server as the settings show it: what the config says and how it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpInfo {
    pub name: String,
    /// The command line or the URL.
    pub target: String,
    pub remote: bool,
    pub state: McpState,
    /// The tools of a ready server, by the name the model calls; empty otherwise.
    pub tools: Vec<McpToolInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolInfo {
    /// `server__tool`, the name its switch goes by.
    pub name: String,
    pub description: String,
    /// On in the chat's mode.
    pub enabled: bool,
}

/// A server that started, with its tools.
pub struct McpServer {
    pub name: String,
    pub client: McpClient,
    pub tools: Vec<RemoteTool>,
}

pub struct McpServers {
    statuses: RwLock<Vec<McpStatus>>,
    /// Starts of each server, so one restarted while it was starting keeps the newer.
    attempts: RwLock<BTreeMap<String, u64>>,
    ready: RwLock<Vec<Arc<McpServer>>>,
    /// True once every server has started or failed.
    settled: watch::Sender<bool>,
    /// Where the tokens of servers signed in to are kept.
    tokens: PathBuf,
}

impl McpServers {
    /// Starts every server the config lists, side by side, without waiting for them.
    pub fn start(config: &Config, tokens: PathBuf) -> Arc<Self> {
        let this = Arc::new(Self {
            statuses: RwLock::default(),
            attempts: RwLock::default(),
            ready: RwLock::default(),
            settled: watch::Sender::new(true),
            tokens,
        });
        for (name, server) in config.mcp() {
            this.restart(name, Some(server.clone()));
        }
        this
    }

    /// Drops the server named `name` and starts `server` in its place; `None`, or a
    /// server switched off, leaves it stopped. Threads opened after it is ready get its
    /// tools; open ones keep what they had.
    pub fn restart(self: &Arc<Self>, name: &str, server: Option<McpServerConfig>) {
        write(&self.ready).retain(|ready| ready.name != name);
        let attempt = {
            let mut attempts = write(&self.attempts);
            let attempt = attempts.entry(name.to_owned()).or_default();
            *attempt += 1;
            *attempt
        };
        let state = match &server {
            None => {
                write(&self.statuses).retain(|status| status.name != name);
                return self.settle();
            }
            Some(server) if !server.enabled() => McpState::Off,
            Some(_) => McpState::Starting,
        };
        let starting = state == McpState::Starting;
        {
            let mut statuses = write(&self.statuses);
            match statuses.iter_mut().find(|status| status.name == name) {
                Some(status) => status.state = state,
                None => statuses.push(McpStatus {
                    name: name.to_owned(),
                    state,
                }),
            }
        }
        self.settle();
        let Some(server) = server.filter(|_| starting) else {
            return;
        };
        let this = Arc::clone(self);
        let name = name.to_owned();
        tokio::spawn(async move {
            let started = connect(&name, &server, this.token_file(&name, &server)).await;
            // Held to the end, so a restart cannot slip in between the check and the update.
            let attempts = read(&this.attempts);
            if attempts.get(&name) != Some(&attempt) {
                return;
            }
            let state = match started {
                Ok(server) => {
                    let tools = server.tools.iter().map(|tool| tool.name.clone()).collect();
                    info!("MCP server {name} is ready");
                    write(&this.ready).push(Arc::new(server));
                    McpState::Ready(tools)
                }
                Err(error) => {
                    let error = bounded(&format!("{error:#}"));
                    warn!("MCP server {name} failed: {error}");
                    McpState::Failed(error)
                }
            };
            this.set_state(&name, state);
            drop(attempts);
        });
    }

    fn token_file(&self, name: &str, server: &McpServerConfig) -> Option<TokenFile> {
        match server {
            McpServerConfig::Http(http) => TokenFile::new(&self.tokens, name, &http.url),
            McpServerConfig::Stdio(_) => None,
        }
    }

    /// Opens the sign-in of the remote server `name`: returns the page for the user's
    /// browser, and starts the server again once they come back from it.
    pub async fn sign_in(self: &Arc<Self>, name: &str, server: McpServerConfig) -> Result<Url> {
        let McpServerConfig::Http(http) = &server else {
            anyhow::bail!("only a remote server signs in");
        };
        let url = check_url(&http.url)?;
        let tokens = self
            .token_file(name, &server)
            .context("the server name cannot name a token file")?;
        let (page, finish) = oauth::sign_in(tokens, &url).await?;
        let this = Arc::clone(self);
        let name = name.to_owned();
        tokio::spawn(async move {
            match finish.await {
                Ok(()) => {
                    info!("signed in to MCP server {name}");
                    this.restart(&name, Some(server));
                }
                Err(error) => {
                    let error = bounded(&format!("{error:#}"));
                    warn!("sign-in to MCP server {name} failed: {error}");
                    this.set_state(&name, McpState::Failed(format!("sign-in: {error}")));
                }
            }
        });
        Ok(page)
    }

    /// The servers that started, for building a thread's tools.
    pub fn ready(&self) -> Vec<Arc<McpServer>> {
        read(&self.ready).clone()
    }

    pub fn statuses(&self) -> Vec<McpStatus> {
        read(&self.statuses).clone()
    }

    /// Every server in `config`, with how it stands.
    pub fn infos(&self, config: &Config) -> Vec<McpInfo> {
        let statuses = read(&self.statuses);
        config
            .mcp()
            .iter()
            .map(|(name, server)| McpInfo {
                name: name.clone(),
                target: server.target(),
                remote: matches!(server, McpServerConfig::Http(_)),
                state: statuses
                    .iter()
                    .find(|status| status.name == *name)
                    .map_or(McpState::Off, |status| status.state.clone()),
                tools: self.tool_infos(name, config),
            })
            .collect()
    }

    fn tool_infos(&self, name: &str, config: &Config) -> Vec<McpToolInfo> {
        let ready = read(&self.ready);
        let Some(server) = ready.iter().find(|server| server.name == name) else {
            return Vec::new();
        };
        server
            .tools
            .iter()
            .filter_map(|tool| {
                let name = McpTool::model_name(&server.name, &tool.name)?;
                Some(McpToolInfo {
                    enabled: config.mode_tool_enabled(config.mode(), &name, ToolKind::Mcp),
                    name,
                    description: tool.description.clone(),
                })
            })
            .collect()
    }

    pub fn starting(&self) -> bool {
        !*self.settled.borrow()
    }

    /// Waits up to `limit` for the servers still starting, so a thread opened right
    /// after the start gets their tools.
    pub async fn wait(&self, limit: Duration) {
        let mut settled = self.settled.subscribe();
        let _ = tokio::time::timeout(limit, settled.wait_for(|settled| *settled)).await;
    }

    fn set_state(&self, name: &str, state: McpState) {
        if let Some(status) = write(&self.statuses)
            .iter_mut()
            .find(|status| status.name == name)
        {
            status.state = state;
        }
        self.settle();
    }

    fn settle(&self) {
        let settled = read(&self.statuses)
            .iter()
            .all(|status| status.state != McpState::Starting);
        self.settled.send_replace(settled);
    }
}

/// Errors can carry a remote server's body; a status line keeps their start.
fn bounded(error: &str) -> String {
    const MAX_ERROR_CHARS: usize = 200;
    match error.char_indices().nth(MAX_ERROR_CHARS) {
        Some((end, _)) => format!("{}\u{2026}", &error[..end]),
        None => error.to_owned(),
    }
}

async fn connect(
    name: &str,
    config: &McpServerConfig,
    tokens: Option<TokenFile>,
) -> anyhow::Result<McpServer> {
    let client = McpClient::start(config, tokens).await?;
    let tools = client.list_tools().await?;
    Ok(McpServer {
        name: name.to_owned(),
        client,
        tools,
    })
}
