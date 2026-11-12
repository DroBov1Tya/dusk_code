//! Providers, MCP servers and plugins of the file.

use super::Config;
use crate::config::{ApiKey, McpServerConfig, Plugged, PluginConfig, ProviderConfig};
use std::collections::BTreeMap;

impl Config {
    /// The providers of the file; [`Config::all_providers`] adds the plugins' ones.
    pub fn providers(&self) -> &BTreeMap<String, ProviderConfig> {
        &self.providers
    }

    /// The providers' API keys, to be moved to a secret store or filled from one.
    pub fn provider_keys_mut(&mut self) -> impl Iterator<Item = (&String, &mut ApiKey)> {
        self.providers
            .iter_mut()
            .map(|(name, provider)| (name, &mut provider.api_key))
    }

    /// A copy that carries no API key: what is written to the file when a keychain keeps them.
    pub fn without_keys(&self) -> Self {
        let mut config = self.clone();
        for (_, key) in config.provider_keys_mut() {
            *key = ApiKey::default();
        }
        config
    }

    /// The MCP servers of the file; [`Config::all_mcp`] adds the plugins' ones.
    pub fn mcp(&self) -> &BTreeMap<String, McpServerConfig> {
        &self.mcp
    }

    /// The file's providers and those of the plugins that are on.
    pub fn all_providers(&self) -> BTreeMap<String, ProviderConfig> {
        let mut all = self.plugged.providers.clone();
        all.extend(self.providers.clone());
        all
    }

    /// The file's MCP servers and those of the plugins that are on.
    pub fn all_mcp(&self) -> BTreeMap<String, McpServerConfig> {
        let mut all = self.plugged.mcp.clone();
        all.extend(self.mcp.clone());
        all
    }

    pub fn plugins(&self) -> &BTreeMap<String, PluginConfig> {
        &self.plugins
    }

    pub fn plugin(&self, name: &str) -> PluginConfig {
        self.plugins.get(name).cloned().unwrap_or_default()
    }

    pub fn set_plugin_enabled(&mut self, name: &str, enabled: bool) {
        self.plugins.entry(name.to_owned()).or_default().enabled = enabled;
    }

    pub fn set_plugged(&mut self, plugged: Plugged) {
        self.plugged = plugged;
    }

    pub fn plugged(&self) -> &Plugged {
        &self.plugged
    }

    /// Adds, replaces or, with `None`, removes an MCP server.
    pub fn set_mcp_server(&mut self, name: &str, server: Option<McpServerConfig>) {
        match server {
            Some(server) => self.mcp.insert(name.to_owned(), server),
            None => self.mcp.remove(name),
        };
    }

    pub fn set_mcp_enabled(&mut self, name: &str, enabled: bool) {
        if let Some(server) = self.mcp.get_mut(name) {
            server.set_enabled(enabled);
        }
    }

    pub fn set_provider(&mut self, name: String, provider: ProviderConfig) {
        self.providers.insert(name, provider);
    }

    /// Whether a provider of the file was found to switch.
    pub fn set_provider_enabled(&mut self, name: &str, enabled: bool) -> bool {
        match self.providers.get_mut(name) {
            Some(provider) => {
                provider.enabled = enabled;
                true
            }
            None => false,
        }
    }

    /// Forgets a provider and what was kept for its models. Whether it is in use is for
    /// the caller to check.
    pub fn remove_provider(&mut self, name: &str) -> bool {
        let prefix = format!("{name}/");
        self.model_contexts
            .retain(|model, _| !model.starts_with(&prefix));
        self.providers.remove(name).is_some()
    }

    /// Forgets every provider and the chosen model, which belonged to one of them.
    pub fn remove_all_providers(&mut self) {
        self.providers.clear();
        self.model_contexts.clear();
        self.model = None;
    }
}
