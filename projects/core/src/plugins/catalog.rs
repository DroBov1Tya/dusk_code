//! The plugins folder, read again every so often like the agents: a plugin dropped in,
//! switched on, edited or removed applies without a restart. New threads get new tools
//! and providers; hooks apply from the next event; platforms start and stop.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Result, ensure};
use parking_lot::{Mutex, RwLock};
use serde_json::json;
use tracing::{info, warn};

use dusk_protocol::plugin::{Part, Permissions, ProjectAccess, Provides};

use super::{
    broker::{self, Broker, Key, Origin, Policy},
    hooks::{Hook, Hooks},
    install,
    isolation::Isolation,
    manifest::{MANIFEST_FILE, Manifest},
    platform::{PlatformSpec, Platforms},
    ui::{UiHost, UiSpec},
};
use crate::{
    config::{McpServerConfig, Plugged, PluginConfig, live::LiveConfig},
    init::paths::{Paths, is_plain_folder_name},
    mcp::McpServers,
};

pub use dusk_protocol::config::PluginInfo;

const PLUGINS_DIR: &str = "plugins";
/// Beside the rules, so updates that replace shipped plugins never touch it.
const DATA_DIR: &str = "plugin-data";
const MAX_SECRET_BYTES: usize = 1024;

/// A secret setting, held in memory only and never printed.
#[derive(Clone)]
struct Secret(String);

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

pub struct Plugins {
    paths: Paths,
    config: Arc<LiveConfig>,
    mcp: Arc<McpServers>,
    hooks: Arc<Hooks>,
    platforms: Arc<Platforms>,
    ui: Arc<UiHost>,
    /// The broker of each plugin that is on and may connect; kept across scans, so its
    /// port and token stay while the plugin runs.
    brokers: Mutex<BTreeMap<String, Broker>>,
    found: RwLock<Vec<PluginInfo>>,
    /// Secret settings by (plugin, key), typed in this session; never saved.
    secrets: Mutex<BTreeMap<(String, String), Secret>>,
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
            ui: Arc::default(),
            brokers: Mutex::default(),
            found: RwLock::default(),
            secrets: Mutex::default(),
            started_mcp: Mutex::default(),
        })
    }

    pub fn ui(&self) -> &Arc<UiHost> {
        &self.ui
    }

    /// What `plugin`'s manifest asks for now, to keep as allowed when it is switched on.
    pub fn permissions(&self, plugin: &str) -> Result<(Permissions, Option<Provides>)> {
        ensure!(is_plain_folder_name(plugin), "invalid plugin name");
        let manifest = Manifest::load(&folder(&self.paths).join(plugin))?;
        Ok((manifest.permissions, manifest.provides))
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

    /// Keeps a secret setting of `plugin` for this session. Call `scan` after, to apply it.
    pub fn set_secret(&self, plugin: &str, key: &str, value: &str) -> Result<()> {
        ensure!(is_plain_folder_name(plugin), "invalid plugin name");
        let manifest = Manifest::load(&folder(&self.paths).join(plugin))?;
        let value = check_secret(&manifest, key, value)?;
        self.secrets
            .lock()
            .insert((plugin.to_owned(), key.to_owned()), Secret(value));
        Ok(())
    }

    /// Reads the folder and applies the plugins that are on.
    pub fn scan(&self) {
        let config = self.config.current();
        let secrets = self.secrets.lock().clone();
        let mut found = Vec::new();
        let mut plugged = Plugged::default();
        let mut hooks = Vec::new();
        let mut platforms = Vec::new();
        let mut interfaces = Vec::new();
        let mut connecting = Vec::new();
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
            let secret_of = |key: &String| secrets.get(&(name.clone(), key.clone()));
            let mut values = settings.settings.clone();
            let mut keys = Vec::new();
            // A key in the file is not read: it is typed in, never saved. Only a key the
            // plugin reads itself is handed to it; the others only its broker puts in.
            for key in &manifest.permissions.keys {
                values.remove(&key.name);
                if let Some(secret) = secret_of(&key.name) {
                    if key.handed {
                        values.insert(key.name.clone(), secret.0.clone());
                    }
                    keys.push((key, secret.0.clone()));
                }
            }
            let missing_settings: Vec<String> = manifest
                .settings
                .iter()
                .chain(manifest.secrets())
                .filter(|key| !values.contains_key(*key) && secret_of(key).is_none())
                .cloned()
                .collect();
            let secret_state: Vec<(String, bool)> = manifest
                .secrets()
                .map(|key| (key.clone(), secret_of(key).is_some()))
                .collect();
            let allowed = allowed(&settings, &manifest);
            let mut problem = None;
            let here = manifest.runs_here();
            if !here {
                let names: Vec<&str> = manifest.platforms.iter().map(|os| os.name()).collect();
                problem = Some(format!("made for {} only", names.join(", ")));
            } else if settings.enabled && !allowed {
                problem = Some("it asks for more than you allowed: switch it on again".into());
            }
            let mut enabled = settings.enabled && missing_settings.is_empty() && allowed && here;
            let mut isolation = None;
            if enabled && manifest.runs_programs() {
                match self.isolate(&manifest, &folder, &values, &keys) {
                    Ok(made) => {
                        if !manifest.permissions.network.is_empty() {
                            connecting.push(name.clone());
                        }
                        isolation = Some(made);
                    }
                    Err(error) => {
                        warn!("plugin {name} cannot be confined: {error:#}");
                        problem = Some(format!("{error:#}"));
                        enabled = false;
                    }
                }
            }
            if let Err(error) = install::sync(&self.paths, &name, enabled.then_some(&*folder)) {
                warn!("cannot install the files of plugin {name}: {error:#}");
            }
            if enabled {
                plugged.providers.extend(manifest.providers.clone());
            }
            if let (true, Some(isolation)) = (enabled, isolation) {
                for (server, mcp) in &manifest.mcp {
                    plugged
                        .mcp
                        .insert(server.clone(), resolve_mcp(mcp, &folder, &isolation));
                }
                hooks.extend(manifest.hooks.iter().map(|hook| Hook {
                    plugin: name.clone(),
                    config: hook.clone(),
                    command: resolve(&hook.command, &folder),
                    isolation: isolation.clone(),
                }));
                if let Some(platform) = &manifest.platform {
                    platforms.push(PlatformSpec {
                        plugin: name.clone(),
                        command: resolve(&platform.command, &folder),
                        isolation: isolation.clone(),
                    });
                }
                if let Some(ui) = &manifest.ui {
                    interfaces.push(ui_spec(&manifest, ui, &folder, isolation));
                }
            }
            let mut info = info(&manifest, &folder, enabled, missing_settings, secret_state);
            if info.problem.is_none() {
                info.problem = problem;
            }
            found.push(info);
        }
        self.brokers
            .lock()
            .retain(|name, _| connecting.contains(name));
        self.apply_mcp(&plugged.mcp);
        self.config.set_plugged(plugged);
        self.hooks.set(hooks);
        self.platforms.set(platforms);
        self.ui.set(interfaces);
        *self.found.write() = found;
    }

    /// The confinement of a plugin's programs, with its broker when it may connect.
    fn isolate(
        &self,
        manifest: &Manifest,
        folder: &Path,
        values: &BTreeMap<String, String>,
        keys: &[(&dusk_protocol::plugin::KeyUse, String)],
    ) -> Result<Isolation> {
        let data = data_folder(&self.paths, folder)?;
        let env = env(folder, &data, values);
        let permissions = &manifest.permissions;
        let isolation = match manifest.provides {
            Some(_) => Isolation::for_contract,
            None => Isolation::new,
        };
        if permissions.network.is_empty() {
            return isolation(&self.paths, folder, &data, permissions, env, None);
        }
        let policy = policy(permissions, keys)?;
        let mut brokers = self.brokers.lock();
        match brokers.get(&manifest.name) {
            Some(broker) => broker.set_policy(policy),
            None => {
                let broker = Broker::start(policy)?;
                brokers.insert(manifest.name.clone(), broker);
            }
        }
        let broker = brokers.get(&manifest.name);
        isolation(&self.paths, folder, &data, permissions, env, broker)
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
                let mut text = serde_json::to_string(server).unwrap_or_default();
                if let McpServerConfig::Stdio(stdio) = server
                    && let Some(isolation) = &stdio.isolation
                {
                    text.push_str(&isolation.fingerprint());
                }
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

pub(super) fn plugin_folders(root: &Path) -> Vec<PathBuf> {
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

/// A private folder for what the plugin keeps, such as a paired owner.
fn data_folder(paths: &Paths, folder: &Path) -> Result<PathBuf> {
    let data = paths.config_root().join(DATA_DIR).join(folder_name(folder));
    crate::init::create_private_dir(&data)?;
    Ok(data)
}

/// What every program of a plugin is given: its folder, its data folder and its
/// settings as JSON, the keys it reads itself among them.
fn env(folder: &Path, data: &Path, settings: &BTreeMap<String, String>) -> Vec<(String, String)> {
    vec![
        ("DUSKCODE_PLUGIN_DIR".into(), folder.display().to_string()),
        ("DUSKCODE_PLUGIN_DATA".into(), data.display().to_string()),
        (
            "DUSKCODE_PLUGIN_SETTINGS".into(),
            json!(settings).to_string(),
        ),
    ]
}

/// `./` begins a path in the plugin's folder.
fn resolve(command: &[String], folder: &Path) -> Vec<String> {
    command
        .iter()
        .map(|word| match word.strip_prefix("./") {
            Some(path) => folder.join(path).display().to_string(),
            None => word.clone(),
        })
        .collect()
}

fn resolve_mcp(server: &McpServerConfig, folder: &Path, isolation: &Isolation) -> McpServerConfig {
    let mut server = server.clone();
    if let McpServerConfig::Stdio(stdio) = &mut server {
        let mut line = vec![stdio.command.clone()];
        line.extend(stdio.args.iter().cloned());
        let mut line = resolve(&line, folder).into_iter();
        stdio.command = line.next().unwrap_or_default();
        stdio.args = line.collect();
        stdio.isolation = Some(isolation.clone());
    }
    server
}

/// Whether what the user agreed to covers all the manifest asks for, permissions and
/// contracts alike.
pub(super) fn allowed(settings: &PluginConfig, manifest: &Manifest) -> bool {
    let permissions = settings
        .allowed
        .as_ref()
        .is_some_and(|allowed| covers(allowed, &manifest.permissions))
        || manifest.permissions == Permissions::default();
    let provides = manifest.provides.as_ref().is_none_or(|wanted| {
        settings
            .allowed_provides
            .as_ref()
            .is_some_and(|agreed| agreed.covers(wanted))
    });
    permissions && provides
}

/// Whether what the user allowed covers all that `wanted` asks for.
pub(super) fn covers(allowed: &Permissions, wanted: &Permissions) -> bool {
    let within = |have: &[String], want: &[String]| want.iter().all(|item| have.contains(item));
    within(&allowed.network, &wanted.network)
        && within(&allowed.exec, &wanted.exec)
        && within(&allowed.read, &wanted.read)
        && within(&allowed.sockets, &wanted.sockets)
        && (wanted.project == ProjectAccess::None || allowed.project == wanted.project)
        && (!wanted.keychain || allowed.keychain)
        && wanted.keys.iter().all(|key| {
            allowed.keys.iter().any(|had| {
                had.name == key.name
                    && within(&had.origins, &key.origins)
                    && (had.handed || !key.handed)
            })
        })
}

/// What the broker lets out: the origins, and each key typed in with where it may go.
fn policy(
    permissions: &Permissions,
    keys: &[(&dusk_protocol::plugin::KeyUse, String)],
) -> Result<Policy> {
    let origins = permissions
        .network
        .iter()
        .map(|origin| Origin::parse(origin))
        .collect::<Result<Vec<_>>>()?;
    let mut granted = Vec::new();
    for (key, value) in keys {
        granted.push(Key {
            name: key.name.clone(),
            value: dusk_protocol::client::Secret(value.clone()),
            origins: key
                .origins
                .iter()
                .map(|origin| Origin::parse(origin))
                .collect::<Result<Vec<_>>>()?,
        });
    }
    Ok(Policy::new(broker::new_token()?, origins, granted))
}

fn ui_spec(
    manifest: &Manifest,
    ui: &super::manifest::Ui,
    folder: &Path,
    isolation: Isolation,
) -> UiSpec {
    let parts = ui
        .parts
        .iter()
        .map(|part| {
            let shown = Part {
                plugin: manifest.name.clone(),
                id: part.id.clone(),
                title: part.title.clone(),
                icon: part.svg.clone(),
                place: part.place,
                opens: part.opens,
                clients: manifest.clients.clone(),
            };
            (shown, part.offered_here())
        })
        .collect();
    UiSpec {
        plugin: manifest.name.clone(),
        command: resolve(&ui.command, folder),
        isolation,
        parts,
    }
}

fn info(
    manifest: &Manifest,
    folder: &Path,
    enabled: bool,
    missing_settings: Vec<String>,
    secrets: Vec<(String, bool)>,
) -> PluginInfo {
    let mut parts = Vec::new();
    for (has, part) in [
        (folder.join("skills").is_dir(), "skills"),
        (folder.join("agents").is_dir(), "agents"),
        (!manifest.providers.is_empty(), "providers"),
        (!manifest.mcp.is_empty(), "mcp"),
        (!manifest.hooks.is_empty(), "hooks"),
        (manifest.platform.is_some(), "platform"),
        (manifest.ui.is_some(), "ui"),
        (manifest.provides.is_some(), "provides"),
    ] {
        if has {
            parts.push(part.to_owned());
        }
    }
    PluginInfo {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        duskcode_version: manifest.duskcode_version.clone(),
        description: manifest.description.clone(),
        enabled,
        parts,
        problem: (!missing_settings.is_empty())
            .then(|| format!("needs settings: {}", missing_settings.join(", "))),
        missing_settings,
        secrets,
        permissions: manifest
            .provides
            .iter()
            .flat_map(Provides::lines)
            .chain(manifest.permissions.lines())
            .collect(),
        platforms: manifest.platforms.clone(),
        clients: manifest.clients.clone(),
    }
}

/// The value to keep for `key`, when the plugin declares it a secret and the value is
/// one line of printable text. Errors never carry the value.
fn check_secret(manifest: &Manifest, key: &str, value: &str) -> Result<String> {
    ensure!(
        manifest.secrets().any(|secret| secret == key),
        "plugin {} has no secret {key}",
        manifest.name
    );
    let value = value.trim();
    ensure!(!value.is_empty(), "the value is empty");
    ensure!(
        value.len() <= MAX_SECRET_BYTES && !value.chars().any(char::is_control),
        "the value is over {MAX_SECRET_BYTES} bytes or has control characters"
    );
    Ok(value.to_owned())
}

fn broken(name: String, enabled: bool, error: &anyhow::Error) -> PluginInfo {
    PluginInfo {
        name,
        version: String::new(),
        duskcode_version: None,
        description: String::new(),
        enabled,
        parts: Vec::new(),
        missing_settings: Vec::new(),
        secrets: Vec::new(),
        permissions: Vec::new(),
        platforms: Vec::new(),
        clients: Vec::new(),
        problem: Some(format!("{error:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        serde_saphyr::from_str(
            "name: bot\nsettings: [chat]\npermissions:\n  network: [https://api.example.com]\n  \
             keys: [{name: token, origins: [https://api.example.com]}]\n",
        )
        .unwrap()
    }

    #[test]
    fn plugin_info_keeps_the_optional_duskcode_version() {
        for (yaml, expected) in [
            ("name: bot\nversion: 1.0.0\n", None),
            (
                "name: bot\nversion: 1.0.0\nduskcode_version: '0.74.2'\n",
                Some("0.74.2"),
            ),
        ] {
            let manifest = serde_saphyr::from_str(yaml).unwrap();
            let plugin = info(&manifest, Path::new("bot"), false, vec![], vec![]);
            let wire = serde_json::to_value(&plugin).unwrap();
            assert_eq!(
                wire.get("duskcode_version").and_then(|v| v.as_str()),
                expected
            );
            let parsed: PluginInfo = serde_json::from_value(wire).unwrap();
            assert_eq!(parsed.version, "1.0.0");
            assert_eq!(parsed.duskcode_version.as_deref(), expected);
        }
    }

    #[test]
    fn only_a_declared_secret_takes_a_clean_value() {
        let manifest = manifest();
        assert_eq!(
            check_secret(&manifest, "token", "  123:abc ").unwrap(),
            "123:abc"
        );
        assert!(
            check_secret(&manifest, "chat", "1").is_err(),
            "not a secret"
        );
        assert!(check_secret(&manifest, "other", "1").is_err());
        assert!(check_secret(&manifest, "token", "  ").is_err());
        assert!(check_secret(&manifest, "token", "a\nb").is_err());
        assert!(check_secret(&manifest, "token", &"x".repeat(MAX_SECRET_BYTES + 1)).is_err());
    }

    /// An update that asks for more stays off until the user allows it again.
    #[test]
    fn allowed_permissions_cover_only_what_was_allowed() {
        let read = |yaml: &str| -> Permissions { serde_saphyr::from_str(yaml).unwrap() };
        let allowed = read(
            "network: [https://a.example.com]\nkeys: [{name: token, origins: [https://a.example.com]}]\n",
        );
        assert!(covers(&allowed, &allowed));
        assert!(covers(&allowed, &Permissions::default()));
        for more in [
            "network: [https://a.example.com, https://b.example.com]\n",
            "exec: [xcrun]\n",
            "read: [~/Library]\n",
            "keys: [{name: token, origins: [https://a.example.com], handed: true}]\n",
            "keys: [{name: other, origins: [https://a.example.com]}]\n",
        ] {
            assert!(!covers(&allowed, &read(more)), "{more}");
        }
    }

    #[test]
    fn an_update_that_serves_more_hosts_stays_off() {
        let manifest = |hosts: &str, permissions: &str| -> Manifest {
            serde_saphyr::from_str(&format!(
                "name: g\npermissions: {{{permissions}}}\nprovides:\n  command: [./p]\n  \
                 forge: {{versions: [1], hosts: [{hosts}]}}\n"
            ))
            .unwrap()
        };
        let agreed = |manifest: &Manifest| PluginConfig {
            enabled: true,
            allowed: Some(manifest.permissions.clone()),
            allowed_provides: manifest.provides.clone(),
            ..Default::default()
        };
        let one = manifest("github.com", "");
        let two = manifest("github.com, ghe.example.com", "");
        assert!(!allowed(&agreed(&one), &two));
        assert!(allowed(&agreed(&two), &two));
        assert!(allowed(&agreed(&two), &one));
        // Asking for the keychain or a project folder is asking for more, too.
        let without = agreed(&one);
        assert!(!allowed(
            &without,
            &manifest("github.com", "keychain: true")
        ));
        assert!(!allowed(&without, &manifest("github.com", "project: read")));
        // A plugin that provides and was never agreed to is off.
        assert!(!allowed(&PluginConfig::default(), &one));
    }

    #[test]
    fn errors_and_debug_hide_the_value() {
        let error = check_secret(&manifest(), "token", "sk\u{7}-secret").unwrap_err();
        assert!(!format!("{error:#}").contains("secret"));
        assert_eq!(format!("{:?}", Secret("sk-secret".into())), "[REDACTED]");
    }

    /// A plugin that serves a contract and connects: on Linux without user namespaces
    /// it does not start, and its row says why.
    #[tokio::test]
    async fn a_contract_plugin_says_why_it_cannot_start() {
        let paths = crate::testing::temp_paths("catalog-contract");
        let plugin = folder(&paths).join("g");
        std::fs::create_dir_all(&plugin).unwrap();
        let provides = "{command: [./p], forge: {versions: [1], hosts: [github.com]}}";
        let permissions = "{network: ['https://api.github.com']}";
        std::fs::write(
            plugin.join(MANIFEST_FILE),
            format!("name: g\npermissions: {permissions}\nprovides: {provides}\n"),
        )
        .unwrap();
        std::fs::write(plugin.join("p"), "#!/bin/sh\n").unwrap();
        let settings = format!(
            "plugins:\n  g:\n    enabled: true\n    allowed: {permissions}\n    \
             allowed_provides: {provides}\n"
        );
        let config = Arc::new(crate::testing::temp_live_config_with(
            "catalog-contract",
            &settings,
        ));
        let mcp = McpServers::start(&config.current(), paths.mcp_tokens_dir());
        let plugins = Plugins::new(paths, config, mcp, Arc::default(), Arc::default());
        plugins.scan();
        let row = plugins
            .list()
            .into_iter()
            .find(|row| row.name == "g")
            .unwrap();
        #[cfg(target_os = "linux")]
        if !crate::platform::netns::available() {
            let problem = row.problem.unwrap_or_default();
            assert!(!row.enabled);
            assert!(
                problem
                    .starts_with("this Linux does not let a program have a network of its own ("),
                "{problem}"
            );
            return;
        }
        if crate::platform::sandbox_available() {
            assert!(row.enabled && row.problem.is_none(), "{row:?}");
        }
    }
}
