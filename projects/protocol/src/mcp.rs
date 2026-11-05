//! The MCP servers the core connects to, and their tools.

use serde::{Deserialize, Serialize};

/// How a server stands, for clients to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpState {
    /// Switched off in the config.
    Off,
    Starting,
    /// The names of its tools, as the server gave them.
    Ready(Vec<String>),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpStatus {
    pub name: String,
    pub state: McpState,
}

/// A server as the settings show it: what the config says and how it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpInfo {
    pub name: String,
    /// The command line or the URL.
    pub target: String,
    pub remote: bool,
    pub state: McpState,
    /// The tools of a ready server, by the name the model calls; empty otherwise.
    pub tools: Vec<McpToolInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolInfo {
    /// `server__tool`, the name its switch goes by.
    pub name: String,
    pub description: String,
    /// On in the chat's mode.
    pub enabled: bool,
}
