//! Plugins and their settings.

use crate::runtime::Runtime;
use anyhow::Result;
use std::sync::Arc;

impl Runtime {
    /// The plugins found, with whether each is on and what keeps one from loading.
    pub fn plugins(&self) -> Vec<crate::plugins::PluginInfo> {
        self.plugins.list()
    }

    /// The parts plugins that are on add to the window, on this system.
    pub fn plugin_parts(&self) -> Vec<dusk_protocol::plugin::Part> {
        self.plugins.ui().parts()
    }

    /// The interfaces of plugins, for the server to open parts in.
    pub fn plugin_ui(&self) -> std::sync::Arc<crate::plugins::UiHost> {
        std::sync::Arc::clone(self.plugins.ui())
    }

    /// Switches a plugin on, allowing what its manifest asks for now, or off; it applies
    /// at once.
    pub fn set_plugin_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        let allowed = match enabled {
            true => Some(self.plugins.permissions(name)?),
            false => None,
        };
        self.config()
            .update(|config| config.set_plugin_enabled(name, allowed))?;
        let plugins = Arc::clone(&self.plugins);
        tokio::task::spawn_blocking(move || plugins.scan());
        Ok(())
    }

    /// Keeps a plugin's secret setting, such as a bot token, for this session only: it
    /// is never written to a file. Applies at once.
    pub fn set_plugin_secret(&self, name: &str, key: &str, value: &str) -> Result<()> {
        self.plugins.set_secret(name, key, value)?;
        let plugins = Arc::clone(&self.plugins);
        tokio::task::spawn_blocking(move || plugins.scan());
        Ok(())
    }
}
