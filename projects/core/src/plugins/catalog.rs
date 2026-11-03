//! The plugins folder, read again every so often like the agents: a plugin dropped in,
//! switched on, edited or removed applies without a restart. New threads get new tools
//! and providers; hooks apply from the next event; platforms start and stop.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use parking_lot::{Mutex, RwLock};
use serde_json::json;
use tracing::{info, warn};

use super::{
    hooks::{Hook, Hooks},
    install,
    manifest::{MANIFEST_FILE, Manifest, SELF_COMMAND},
    platform::{PlatformSpec, Platforms},
};
use crate::{
    config::{McpServerConfig, Plugged, live::LiveConfig},
    init::paths::{Paths, is_plain_folder_name},
    mcp::McpServers,
};

const PLUGINS_DIR: &str = "plugins";
/// Beside the rules, so updates that replace shipped plugins never touch it.
const DATA_DIR: &str = "plugin-data";

/// A plugin as `/settings` shows it.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    /// What it brings, such as `mcp` or `hooks`.
    pub parts: Vec<&'static str>,
    /// Settings it asks for that the config does not give.
    pub missing_settings: Vec<String>,
    /// Why it cannot load, when it cannot.
    pub problem: Option<String>,
}

pub struct Plugins {
    paths: Paths,
    config: Arc<LiveConfig>,
    mcp: Arc<McpServers>,
    hooks: Arc<Hooks>,
    platforms: Arc<Platforms>,
    found: RwLock<Vec<PluginInfo>>,
    /// The MCP servers plugins started, as last applied, to tell what changed.
    started_mcp: Mutex<BTreeMap<String, String>>,
}

/// `rules/plugins/`, where built-in plugins are shipped and the user's are dropped.
pub fn folder(paths: &Paths) -> PathBuf {
    paths.rules_root().join(PLUGINS_DIR)
}

impl Plugins {
    pub fn new(
        paths: Paths,
        config: Arc<LiveConfig>,
        mcp: Arc<McpServers>,
        hooks: Arc<Hooks>,
        platforms: Arc<Platforms>,
    ) -> Arc<Self> {
        Arc::new(Self {
            paths,
            config,
            mcp,
            hooks,
            platforms,
            found: RwLock::default(),
            started_mcp: Mutex::default(),
        })
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        let mut list = self.found.read().clone();
        for plugin in &mut list {
            if plugin.enabled
                && let Some(status) = self.platforms.status(&plugin.name)
            {
                plugin.problem.get_or_insert(status);
            }
        }
        list
    }

    /// Reads the folder and applies the plugins that are on.
    pub fn scan(&self) {
        let config = self.config.current();
        let mut found = Vec::new();
        let mut plugged = Plugged::default();
        let mut hooks = Vec::new();
        let mut platforms = Vec::new();
        for folder in plugin_folders(&folder(&self.paths)) {
            let name = folder_name(&folder);
            let settings = config.plugin(&name);
            let manifest = match Manifest::load(&folder) {
                Ok(manifest) => manifest,
                Err(error) => {
                    found.push(broken(name, settings.enabled, &error));
                    continue;
                }
            };
            let missing_settings: Vec<String> = manifest
                .settings
                .iter()
                .filter(|key| !settings.settings.contains_key(*key))
                .cloned()
                .collect();
            let enabled = settings.enabled && missing_settings.is_empty();
            let env = env(&self.paths, &folder, &settings.settings);
            if let Err(error) = install::sync(&self.paths, &name, enabled.then_some(&*folder)) {
                warn!("cannot install the files of plugin {name}: {error:#}");
            }
            if enabled {
                plugged.providers.extend(manifest.providers.clone());
                for (server, mcp) in &manifest.mcp {
                    plugged
                        .mcp
                        .insert(server.clone(), resolve_mcp(mcp, &folder, &env));
                }
                hooks.extend(manifest.hooks.iter().map(|hook| Hook {
                    plugin: name.clone(),
                    config: hook.clone(),
                    command: resolve(&hook.command, &folder),
                    folder: folder.clone(),
                    env: env.clone(),
                }));
                if let Some(platform) = &manifest.platform {
                    platforms.push(PlatformSpec {
                        plugin: name.clone(),
                        command: resolve(&platform.command, &folder),
                        folder: folder.clone(),
                        env: env.clone(),
                    });
                }
            }
            found.push(info(&manifest, &folder, enabled, missing_settings));
        }
        self.apply_mcp(&plugged.mcp);
        self.config.set_plugged(plugged);
        self.hooks.set(hooks);
        self.platforms.set(platforms);
        *self.found.write() = found;
    }

    /// Reads the folder again every `period`.
    pub async fn watch(self: Arc<Self>, period: Duration) {
        let mut ticks = tokio::time::interval(period);
        ticks.tick().await;
        loop {
            ticks.tick().await;
            let plugins = Arc::clone(&self);
            if let Err(error) = tokio::task::spawn_blocking(move || plugins.scan()).await {
                warn!("cannot read the plugins: {error}");
            }
        }
    }

    /// Starts the plugins' servers that are new or changed and stops those that went; a
    /// server of the config file of the same name is the config's and left alone.
    fn apply_mcp(&self, wanted: &BTreeMap<String, McpServerConfig>) {
        let own = self.config.current().mcp().clone();
        let mut started = self.started_mcp.lock();
        let wanted: BTreeMap<String, (String, McpServerConfig)> = wanted
            .iter()
            .filter(|(name, _)| !own.contains_key(*name))
            .map(|(name, server)| {
                let text = serde_json::to_string(server).unwrap_or_default();
                (name.clone(), (text, server.clone()))
            })
            .collect();
        for name in started.keys() {
            if !wanted.contains_key(name) && !own.contains_key(name) {
                self.mcp.restart(name, None);
            }
        }
        for (name, (text, server)) in &wanted {
            if started.get(name) != Some(text) {
                info!("plugin MCP server {name} starts");
                self.mcp.restart(name, Some(server.clone()));
            }
        }
        *started = wanted
            .into_iter()
            .map(|(name, (text, _))| (name, text))
            .collect();
    }
}

fn plugin_folders(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| entry.file_name().to_str().is_some_and(is_plain_folder_name))
        .filter(|entry| entry.path().join(MANIFEST_FILE).is_file())
        .map(|entry| entry.path())
        .collect();
    folders.sort();
    folders
}

fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// What every program of a plugin is given: its folder, a private folder for what it
/// keeps, such as a paired owner, and its settings as JSON.
fn env(paths: &Paths, folder: &Path, settings: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let data = paths.config_root().join(DATA_DIR).join(folder_name(folder));
    if let Err(error) = crate::init::create_private_dir(&data) {
        warn!("cannot make the data folder of a plugin: {error:#}");
    }
    vec![
        ("DUSKCODE_PLUGIN_DIR".into(), folder.display().to_string()),
        ("DUSKCODE_PLUGIN_DATA".into(), data.display().to_string()),
        (
            "DUSKCODE_PLUGIN_SETTINGS".into(),
            json!(settings).to_string(),
        ),
    ]
}

/// `duskcode` first stands for this binary; `./` begins a path in the plugin's folder.
fn resolve(command: &[String], folder: &Path) -> Vec<String> {
    command
        .iter()
        .enumerate()
        .map(|(index, word)| match word.strip_prefix("./") {
            _ if index == 0 && word == SELF_COMMAND => std::env::current_exe()
                .map(|exe| exe.display().to_string())
                .unwrap_or_else(|_| word.clone()),
            Some(path) => folder.join(path).display().to_string(),
            None => word.clone(),
        })
        .collect()
}

fn resolve_mcp(
    server: &McpServerConfig,
    folder: &Path,
    env: &[(String, String)],
) -> McpServerConfig {
    let mut server = server.clone();
    if let McpServerConfig::Stdio(stdio) = &mut server {
        let mut line = vec![stdio.command.clone()];
        line.extend(stdio.args.iter().cloned());
        let mut line = resolve(&line, folder).into_iter();
        stdio.command = line.next().unwrap_or_default();
        stdio.args = line.collect();
        stdio.env.extend(env.iter().cloned());
    }
    server
}

fn info(
    manifest: &Manifest,
    folder: &Path,
    enabled: bool,
    missing_settings: Vec<String>,
) -> PluginInfo {
    let mut parts = Vec::new();
    for (has, part) in [
        (folder.join("skills").is_dir(), "skills"),
        (folder.join("agents").is_dir(), "agents"),
        (!manifest.providers.is_empty(), "providers"),
        (!manifest.mcp.is_empty(), "mcp"),
        (!manifest.hooks.is_empty(), "hooks"),
        (manifest.platform.is_some(), "platform"),
    ] {
        if has {
            parts.push(part);
        }
    }
    PluginInfo {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        enabled,
        parts,
        problem: (!missing_settings.is_empty())
            .then(|| format!("needs settings: {}", missing_settings.join(", "))),
        missing_settings,
    }
}

fn broken(name: String, enabled: bool, error: &anyhow::Error) -> PluginInfo {
    PluginInfo {
        name,
        version: String::new(),
        description: String::new(),
        enabled,
        parts: Vec::new(),
        missing_settings: Vec::new(),
        problem: Some(format!("{error:#}")),
    }
}
