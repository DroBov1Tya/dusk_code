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
    pub fn plugin_parts(&self) -> Arc<Vec<dusk_protocol::plugin::Part>> {
        self.plugins.ui().parts()
    }

    /// The interfaces of plugins, for the server to open parts in.
    pub fn plugin_ui(&self) -> std::sync::Arc<crate::plugins::UiHost> {
        std::sync::Arc::clone(self.plugins.ui())
    }

    /// Lists, installs or removes plugins of the store. An installed plugin is switched
    /// on, allowing what its listing showed, which its manifest was checked to match.
    pub async fn store(
        &self,
        request: dusk_protocol::plugin::StoreRequest,
    ) -> Result<dusk_protocol::client::Response> {
        use crate::plugins::store;
        use dusk_protocol::{client::Response, plugin::StoreRequest};
        match request {
            StoreRequest::List => return Ok(Response::Store(store::list(self.paths()).await?)),
            StoreRequest::Install { name, version } => {
                store::install(self.paths(), &name, &version).await?;
                tracing::info!("plugin {name} {version} installed from the store");
                self.set_plugin_enabled(&name, true)?;
            }
            StoreRequest::Remove { name } => {
                self.set_plugin_enabled(&name, false)?;
                store::remove(self.paths(), &name)?;
                tracing::info!("plugin {name} removed");
                let plugins = Arc::clone(&self.plugins);
                tokio::task::spawn_blocking(move || plugins.scan());
            }
        }
        Ok(Response::Done)
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
