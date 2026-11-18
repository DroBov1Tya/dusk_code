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

    pub async fn scan_plugins(&self) -> Result<()> {
        let plugins = Arc::clone(&self.plugins);
        tokio::task::spawn_blocking(move || plugins.scan()).await?;
        Ok(())
    }

    /// Lists, installs, updates or removes plugins of the store. An installed plugin is
    /// switched on, allowing what its listing showed, which its manifest was checked to
    /// match; an updated one keeps its switch, since it asks for nothing more.
    pub async fn store(
        &self,
        request: dusk_protocol::plugin::StoreRequest,
    ) -> Result<dusk_protocol::client::Response> {
        use crate::plugins::store;
        use dusk_protocol::{
            client::Response,
            plugin::{Kind, StoreRequest},
        };
        let mirrors = self.config().current().workshop().sources.clone();
        match request {
            StoreRequest::List { order, category } => {
                let list = store::list(self.paths(), &mirrors, order, category.as_deref()).await?;
                return Ok(Response::Store(list));
            }
            StoreRequest::Install {
                source,
                tier,
                name,
                version,
                agreed,
            } => {
                let agreed = store::Agreed {
                    source: &source,
                    tier,
                    name: &name,
                    version: &version,
                    permissions: &agreed,
                };
                let kind = store::install(self.paths(), &mirrors, &agreed).await?;
                tracing::info!("{kind:?} {name} {version} installed from {source}");
                if kind == Kind::Plugin {
                    self.set_plugin_enabled(&name, true)?;
                }
            }
            StoreRequest::Remove { name } => {
                let kind = store::check_removable(self.paths(), &name)?;
                if kind == Kind::Plugin {
                    self.set_plugin_enabled(&name, false)?;
                }
                store::remove(self.paths(), &name).await?;
                tracing::info!("{kind:?} {name} removed");
                let plugins = Arc::clone(&self.plugins);
                tokio::task::spawn_blocking(move || plugins.scan());
            }
            StoreRequest::UpdateAll => {
                let report = store::update_all(self.paths(), &mirrors).await?;
                for line in &report.updated {
                    tracing::info!("plugin {line} updated from the store");
                }
                let plugins = Arc::clone(&self.plugins);
                tokio::task::spawn_blocking(move || plugins.scan());
                return Ok(Response::StoreUpdates(report));
            }
            StoreRequest::Cover { source, name } => {
                use base64::Engine;
                let (bytes, mime) = store::cover(&mirrors, &source, &name).await?;
                let base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                let mime = mime.to_owned();
                return Ok(Response::Media { base64, mime });
            }
            StoreRequest::Screenshot {
                source,
                name,
                index,
            } => {
                use base64::Engine;
                let (bytes, mime) = store::screenshot(&mirrors, &source, &name, index).await?;
                let base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                let mime = mime.to_owned();
                return Ok(Response::Media { base64, mime });
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
