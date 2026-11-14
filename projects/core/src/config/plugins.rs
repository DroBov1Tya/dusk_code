//! The `plugins:` section: which plugins are on and the settings each one is given, and
//! what the plugins that are on add to the config while the core runs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{McpServerConfig, ProviderConfig};

/// A plugin as the user set it up. Found plugins stay off until switched on, since a
/// plugin runs programs of its own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Handed to the plugin's programs, such as a bot's token.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, String>,
    /// What the user allowed when switching it on; an update that asks for more keeps
    /// it off until it is switched on again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed: Option<dusk_protocol::plugin::Permissions>,
}

/// What the plugins that are on add: kept in memory only, never written to the file.
/// An entry of the file wins over a plugin's of the same name.
#[derive(Clone, Default)]
pub struct Plugged {
    pub providers: BTreeMap<String, ProviderConfig>,
    pub mcp: BTreeMap<String, McpServerConfig>,
}
