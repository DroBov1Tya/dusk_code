//! The MCP servers the core connects to, and their tools.

use serde::{Deserialize, Serialize};

/// How a server stands, for clients to show.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpState {
    /// Switched off in the config.
    Off,
    Starting,
    /// The names of its tools, as the server gave them.
    Ready(Vec<String>),
    Failed(String),
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpStatus {
    pub name: String,
    pub state: McpState,
}

/// A server as the settings show it: what the config says and how it stands.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpInfo {
    pub name: String,
    /// The command line or the URL.
    pub target: String,
    pub remote: bool,
    pub state: McpState,
    /// The tools of a ready server, by the name the model calls; empty otherwise.
    pub tools: Vec<McpToolInfo>,
    /// Its picture, a file the core found in the config's `icons` folder.
    #[serde(default)]
    pub icon: Option<std::path::PathBuf>,
    /// The Workshop source that added it and the tier its listing had, while it is the
    /// server that source added; none for a server set up by hand or a plugin's.
    #[serde(default)]
    pub added_from: Option<AddedFrom>,
}

/// Where a store-added MCP server came from, shown beside it.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddedFrom {
    pub source: String,
    pub tier: crate::plugin::Tier,
}

impl AddedFrom {
    /// How every client words it.
    pub fn label(&self) -> String {
        let tier = match self.tier {
            crate::plugin::Tier::Official => "Official",
            crate::plugin::Tier::Verified => "Verified",
            crate::plugin::Tier::Community => "Community",
            crate::plugin::Tier::Unverified => "Unverified",
            crate::plugin::Tier::Local => "Local",
        };
        format!("From {} \u{b7} {tier}", self.source)
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolInfo {
    /// `server__tool`, the name its switch goes by.
    pub name: String,
    /// The tool's own name on its server, made a plain line by the core, to show; `name`
    /// holds it changed to fit a model.
    /// Empty from a core older than protocol 37.
    #[serde(default)]
    pub short_name: String,
    pub description: String,
    /// On in the chat's mode.
    pub enabled: bool,
}
