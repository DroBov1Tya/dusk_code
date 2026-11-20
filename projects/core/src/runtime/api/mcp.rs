//! MCP servers.

use crate::runtime::Runtime;
use crate::{
    config::{McpServerConfig, mcp_server_yaml, parse_mcp_server},
    mcp::{McpInfo, McpStatus},
};
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

impl Runtime {
    /// How each MCP server in the config stands.
    pub fn mcp_statuses(&self) -> Vec<McpStatus> {
        self.launcher.mcp.statuses()
    }

    /// Every MCP server in the config, with how it stands.
    pub fn mcp_servers(&self) -> Vec<McpInfo> {
        let config = self.launcher.config.current();
        let icons = self.launcher.paths.config_root().join(ICONS_DIR);
        let mut infos = self.launcher.mcp.infos(&config);
        let servers = config.all_mcp();
        let added = crate::plugins::store::connector::all_added(self.paths());
        for info in &mut infos {
            let server = servers.get(&info.name);
            let name = server.and_then(|server| server.icon());
            info.icon = name.and_then(|name| icon_file(&icons, name));
            // Only the user's own servers can be one the store added, and only unchanged.
            let own = config.mcp().get(&info.name);
            info.added_from = added.get(&info.name).and_then(|added| {
                let same = own.is_some_and(|server| {
                    crate::plugins::store::connector::target(server) == added.target
                });
                same.then(|| dusk_protocol::config::AddedFrom {
                    source: added.source.clone(),
                    tier: added
                        .tier
                        .unwrap_or(dusk_protocol::plugin::Tier::Unverified),
                })
            });
        }
        infos
    }

    /// One MCP server as YAML to edit, `name:` and its fields.
    pub fn mcp_server_text(&self, name: &str) -> Result<String> {
        let config = self.launcher.config.current();
        let server = config
            .mcp()
            .get(name)
            .with_context(|| format!("no MCP server {name}"))?;
        mcp_server_yaml(name, server)
    }

    /// Saves one MCP server from its YAML in place of `old`, and starts it again. Empty
    /// text removes `old`. Returns the name saved.
    pub fn save_mcp_server(&self, old: Option<&str>, text: &str) -> Result<Option<String>> {
        if text.trim().is_empty() {
            let old = old.context("write the server's YAML")?;
            self.launcher
                .config
                .update(|config| config.set_mcp_server(old, None))?;
            self.launcher.mcp.restart(old, None);
            self.forget_connector(old, None)?;
            return Ok(None);
        }
        let (name, server) = parse_mcp_server(text)?;
        if let Some(old) = old {
            let kept = (old == name).then_some(&server);
            self.forget_connector(old, kept)?;
        }
        self.put_mcp_server(old, &name, server)?;
        Ok(Some(name))
    }

    /// A server the store added stops being the store's once the user removes it,
    /// renames it or points it elsewhere in the settings (`kept` is what it becomes, under
    /// its own name): the Workshop then never shows a server set up by hand as Added, and
    /// its Remove never deletes one.
    fn forget_connector(&self, name: &str, kept: Option<&McpServerConfig>) -> Result<()> {
        use crate::plugins::store::connector;
        let Some(added) = connector::added(self.paths(), name)? else {
            return Ok(());
        };
        if kept.is_none_or(|server| connector::target(server) != added.target) {
            connector::forget(self.paths(), name)?;
        }
        Ok(())
    }

    /// Writes `server` as `name` in place of `old` and starts it again: the one way an
    /// MCP server gets into the config, from the settings or from a Workshop connector.
    pub fn put_mcp_server(
        &self,
        old: Option<&str>,
        name: &str,
        server: McpServerConfig,
    ) -> Result<()> {
        let config = self.launcher.config.current();
        ensure!(
            old == Some(name) || !config.mcp().contains_key(name),
            "MCP server {name} already exists"
        );
        self.launcher.config.update(|config| {
            if let Some(old) = old {
                config.set_mcp_server(old, None);
            }
            config.set_mcp_server(name, Some(server.clone()));
        })?;
        if let Some(old) = old.filter(|old| *old != name) {
            self.launcher.mcp.restart(old, None);
        }
        self.launcher.mcp.restart(name, Some(server));
        Ok(())
    }

    pub fn set_mcp_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        let config = self
            .launcher
            .config
            .update(|config| config.set_mcp_enabled(name, enabled))?;
        self.launcher
            .mcp
            .restart(name, config.mcp().get(name).cloned());
        Ok(())
    }

    /// Starts an MCP server again, as the config has it now.
    pub fn restart_mcp(&self, name: &str) {
        let config = self.launcher.config.current();
        self.launcher
            .mcp
            .restart(name, config.mcp().get(name).cloned());
    }

    /// Starts signing in to a remote MCP server: the page to open in the browser. The
    /// server starts again once the user comes back from it.
    pub async fn sign_in_mcp(&self, name: &str) -> Result<reqwest::Url> {
        let config = self.launcher.config.current();
        let server = config
            .mcp()
            .get(name)
            .with_context(|| format!("no MCP server {name}"))?;
        self.launcher.mcp.sign_in(name, server.clone()).await
    }

    /// Whether an MCP server is still starting.
    pub fn mcp_starting(&self) -> bool {
        self.launcher.mcp.starting()
    }
}

/// Where the pictures of MCP servers are kept, under the config folder.
const ICONS_DIR: &str = "icons";

/// The picture `name` in `icons`: a plain file name of a PNG, JPEG or SVG that is a
/// regular file there, never a path out of the folder or a link.
fn icon_file(icons: &Path, name: &str) -> Option<PathBuf> {
    let (stem, extension) = name.rsplit_once('.')?;
    let picture = matches!(
        extension.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "svg"
    );
    if !picture || !crate::init::paths::is_plain_folder_name(stem) {
        return None;
    }
    let path = icons.join(name);
    std::fs::symlink_metadata(&path)
        .is_ok_and(|metadata| metadata.is_file())
        .then_some(path)
}

#[cfg(test)]
mod icon_tests {
    use super::*;

    #[test]
    fn an_icon_is_a_plain_picture_in_its_folder() {
        let icons = crate::testing::temp_dir("mcp_icons");
        std::fs::create_dir_all(&icons).unwrap();
        std::fs::write(icons.join("acme.png"), b"png").unwrap();
        std::fs::write(icons.join("notes.txt"), b"text").unwrap();
        assert_eq!(icon_file(&icons, "acme.png"), Some(icons.join("acme.png")));
        for name in ["../acme.png", "notes.txt", "missing.png", "a/b.png", ".png"] {
            assert_eq!(icon_file(&icons, name), None, "{name}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/hosts", icons.join("link.png")).unwrap();
            assert_eq!(icon_file(&icons, "link.png"), None);
        }
    }
}
