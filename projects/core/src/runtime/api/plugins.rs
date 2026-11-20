//! Plugins and their settings.

#[cfg(test)]
mod connector_tests;

use crate::runtime::Runtime;
use anyhow::{Result, ensure};
use std::sync::Arc;

impl Runtime {
    /// The plugins found, with whether each is on and what keeps one from loading.
    pub fn plugins(&self) -> Vec<crate::plugins::PluginInfo> {
        self.plugins.list()
    }

    /// The plugins that serve contracts, and the routing to them.
    pub fn contracts(&self) -> Arc<crate::plugins::contract::Contracts> {
        Arc::clone(self.plugins.contracts())
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
            StoreRequest::List {
                order,
                category,
                kinds,
            } => {
                let on = self
                    .plugins
                    .list()
                    .into_iter()
                    .filter(|plugin| plugin.enabled)
                    .map(|plugin| plugin.name)
                    .collect();
                let here = store::Here::of(&self.config().current(), self.paths(), on);
                let category = category.as_deref();
                let list =
                    store::list(self.paths(), &mirrors, order, category, &kinds, &here).await?;
                return Ok(Response::Store(list));
            }
            StoreRequest::Install {
                source,
                tier,
                name,
                version,
                agreed,
                sha256,
            } => {
                let agreed = store::Agreed {
                    source: &source,
                    tier,
                    name: &name,
                    version: &version,
                    permissions: &agreed,
                    sha256: sha256.as_deref(),
                };
                match store::install(self.paths(), &mirrors, &agreed).await? {
                    store::Placed::Files(kind) => {
                        tracing::info!("{kind:?} {name} {version} installed from {source}");
                        if kind == Kind::Plugin {
                            self.set_plugin_enabled(&name, true)?;
                        }
                    }
                    store::Placed::Server { config, added } => {
                        self.add_connector(&name, config, *added)?;
                        tracing::info!("connector {name} {version} added from {source}");
                    }
                }
            }
            StoreRequest::Remove { name } if self.remove_connector(&name)? => {
                tracing::info!("connector {name} removed");
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
            StoreRequest::Contents { source, name } => {
                let contents =
                    store::contents::contents(self.paths(), &mirrors, &source, &name).await?;
                return Ok(Response::StoreContents(contents));
            }
            StoreRequest::Review { source, name } => {
                let review = store::review::review(self.paths(), &mirrors, &source, &name).await?;
                return Ok(Response::StoreReview(review));
            }
            StoreRequest::File { source, name, path } => {
                let file = store::contents::file(&source, &name, &path).await?;
                return Ok(Response::StoreFile(file));
            }
            StoreRequest::Probe { url, key } => {
                let (mirror, index) = store::probe(&url, &key).await?;
                return Ok(Response::StoreProbe(store::probed(&mirror, &index)));
            }
            StoreRequest::Sources => {
                let rows = store::sources(self.paths(), &mirrors).await;
                return Ok(Response::StoreSources(rows));
            }
            StoreRequest::AddSource { url, key } => {
                // The client's probe is not trusted: the list is read again here.
                let mirror = store::add::proposed(self.config(), &url, &key, None)?;
                store::add::add(self.config(), mirror).await?;
            }
            StoreRequest::RemoveSource { name } => {
                anyhow::ensure!(
                    mirrors.iter().any(|item| item.name == name),
                    "there is no repository named {name}"
                );
                self.config().update(|config| {
                    config
                        .workshop_mut()
                        .sources
                        .retain(|item| item.name != name)
                })?;
                tracing::info!("workshop repository {name} removed");
            }
        }
        Ok(Response::Done)
    }

    /// Adds a connector's server through the settings' own path. The same connector added
    /// before is replaced, keeping the secrets the user filled in while it still goes to
    /// the same address or program; a server of the user's under its name is never
    /// touched.
    fn add_connector(
        &self,
        name: &str,
        mut server: crate::config::McpServerConfig,
        added: crate::plugins::store::Installed,
    ) -> Result<()> {
        use crate::plugins::store::{connector, listing_plugin_servers};
        ensure!(
            !listing_plugin_servers(self.paths()).contains(name),
            "a plugin here brings an MCP server named {name}; a connector may not take it"
        );
        let config = self.config().current();
        let old = match config.mcp().get(name) {
            None => None,
            Some(old) => {
                // Ours only when this source added exactly this server: a server the
                // user set up by hand, or another source's, is never replaced.
                let ours = connector::added(self.paths(), name)?.is_some_and(|before| {
                    before.source == added.source
                        && before.key == added.key
                        && before.url == added.url
                        && before.target == connector::target(old)
                });
                ensure!(
                    ours,
                    "a connector of yours is named {name}; rename or remove it to add this one"
                );
                // Keys the user typed went to that address or program; one the listing
                // moved elsewhere starts without them.
                if connector::target(&server) == connector::target(old) {
                    connector::keep_secrets(&mut server, old);
                }
                Some(name)
            }
        };
        self.put_mcp_server(old, name, server)?;
        connector::remember(self.paths(), name, added)
    }

    /// Removes the connector the store added as `name`, when it added one: its server goes
    /// unless the user changed it since, and the record of it goes either way. Answers
    /// whether `name` was a connector.
    fn remove_connector(&self, name: &str) -> Result<bool> {
        use crate::plugins::store::connector;
        let Some(added) = connector::added(self.paths(), name)? else {
            return Ok(false);
        };
        let config = self.config().current();
        if config
            .mcp()
            .get(name)
            .is_some_and(|server| connector::target(server) == added.target)
        {
            self.save_mcp_server(Some(name), "")?;
        }
        connector::forget(self.paths(), name)?;
        Ok(true)
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

    /// Chooses the plugin a contract goes to for one host; applies at once.
    pub fn set_contract_plugin(&self, contract: &str, host: &str, plugin: &str) -> Result<()> {
        ensure!(contract == "forge", "unknown contract {contract}");
        ensure!(
            crate::plugins::manifest::is_forge_host(host),
            "not a forge host: {host}"
        );
        ensure!(
            crate::init::paths::is_plain_folder_name(plugin),
            "invalid plugin name"
        );
        self.config()
            .update(|config| config.set_contract_plugin(contract, host, plugin))?;
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
