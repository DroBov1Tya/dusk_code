//! MCP servers the config lists. Each is started in the background when the core starts;
//! its tools join every thread started after it is ready. A server that fails stays
//! failed until the next start, and says why.

mod client;

use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use tokio::sync::watch;
use tracing::{info, warn};

pub use client::{McpClient, RemoteTool};

use crate::config::Config;

/// How a server stands, for clients to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpState {
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

/// A server that started, with its tools.
pub struct McpServer {
    pub name: String,
    pub client: McpClient,
    pub tools: Vec<RemoteTool>,
}

pub struct McpServers {
    statuses: RwLock<Vec<McpStatus>>,
    ready: RwLock<Vec<Arc<McpServer>>>,
    /// True once every server has started or failed.
    settled: watch::Sender<bool>,
}

impl McpServers {
    /// Starts every server the config lists, side by side, without waiting for them.
    pub fn start(config: &Config) -> Arc<Self> {
        let servers: Vec<_> = config
            .mcp()
            .iter()
            .map(|(name, server)| (name.clone(), server.clone()))
            .collect();
        let statuses = servers
            .iter()
            .map(|(name, _)| McpStatus {
                name: name.clone(),
                state: McpState::Starting,
            })
            .collect();
        let this = Arc::new(Self {
            statuses: RwLock::new(statuses),
            ready: RwLock::default(),
            settled: watch::Sender::new(servers.is_empty()),
        });
        for (name, server) in servers {
            let this = Arc::clone(&this);
            tokio::spawn(async move {
                let state = match connect(&name, &server).await {
                    Ok(server) => {
                        let tools = server.tools.iter().map(|tool| tool.name.clone()).collect();
                        info!("MCP server {name} is ready");
                        write(&this.ready).push(Arc::new(server));
                        McpState::Ready(tools)
                    }
                    Err(error) => {
                        warn!("MCP server {name} failed: {error:#}");
                        McpState::Failed(format!("{error:#}"))
                    }
                };
                this.set_state(&name, state);
            });
        }
        this
    }

    /// The servers that started, for building a thread's tools.
    pub fn ready(&self) -> Vec<Arc<McpServer>> {
        read(&self.ready).clone()
    }

    pub fn statuses(&self) -> Vec<McpStatus> {
        read(&self.statuses).clone()
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
        let mut statuses = write(&self.statuses);
        if let Some(status) = statuses.iter_mut().find(|status| status.name == name) {
            status.state = state;
        }
        let settled = statuses
            .iter()
            .all(|status| status.state != McpState::Starting);
        drop(statuses);
        self.settled.send_replace(settled);
    }
}

async fn connect(name: &str, config: &crate::config::McpServerConfig) -> anyhow::Result<McpServer> {
    let client = McpClient::start(config).await?;
    let tools = client.list_tools().await?;
    Ok(McpServer {
        name: name.to_owned(),
        client,
        tools,
    })
}

// Writes replace whole values, so a panic cannot leave them half-changed.
fn read<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
