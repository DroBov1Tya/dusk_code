//! The plugins found at runtime, as the plugins window lists them.

use serde::{Deserialize, Serialize};

/// A plugin as `/settings` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    /// What it brings, such as `mcp` or `hooks`.
    pub parts: Vec<String>,
    /// Settings it asks for that the config does not give.
    pub missing_settings: Vec<String>,
    /// Its secret settings and whether each is set for this session.
    pub secrets: Vec<(String, bool)>,
    /// Why it cannot load, when it cannot.
    pub problem: Option<String>,
}
