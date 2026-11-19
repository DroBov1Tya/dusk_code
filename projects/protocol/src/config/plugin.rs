//! The plugins found at runtime, as the plugins window lists them.

use serde::{Deserialize, Serialize};

/// A plugin as `/settings` shows it.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    /// The Dusk Code version it was written for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duskcode_version: Option<String>,
    pub description: String,
    pub enabled: bool,
    /// What it brings, such as `mcp` or `hooks`.
    pub parts: Vec<String>,
    /// Settings it asks for that the config does not give.
    pub missing_settings: Vec<String>,
    /// Its secret settings and whether each is set for this session.
    pub secrets: Vec<(String, bool)>,
    /// What it may do, one line each, as the user allows it by switching it on.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// The systems it runs on; all when empty.
    #[serde(default)]
    pub platforms: Vec<crate::plugin::Os>,
    /// The clients that can draw its parts; all when empty.
    #[serde(default)]
    pub clients: Vec<crate::plugin::Client>,
    /// Hosts it serves where another plugin that is on serves them too and this one is
    /// not the routed one: the client offers "Use for {host}" for each.
    #[serde(default)]
    pub choose: Vec<String>,
    /// Why it cannot load, when it cannot.
    pub problem: Option<String>,
}
