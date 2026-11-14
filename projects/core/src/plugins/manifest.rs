//! `plugin.yml`, what a plugin says it brings. Unknown fields are refused, so a typo
//! shows up as an error instead of a part that silently does nothing.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
use dusk_protocol::plugin::{Client, Opens, Os, Permissions, Place, allows, runs_here};
use serde::Deserialize;

use super::{broker::Origin, ui::icon};
use crate::config::{McpServerConfig, ProviderConfig};

pub const MANIFEST_FILE: &str = "plugin.yml";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_TITLE: usize = 60;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The same as the plugin's folder.
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// The systems its programs are built for; all when empty.
    #[serde(default)]
    pub platforms: Vec<Os>,
    /// The clients that can draw its parts; all when empty.
    #[serde(default)]
    pub clients: Vec<Client>,
    /// Settings the user gives it under `plugins:` in the config, such as a token.
    #[serde(default)]
    pub settings: Vec<String>,
    /// What it may do besides running in its own folder; its `keys` are settings that are
    /// secrets, typed in each session and never written to any file.
    #[serde(default)]
    pub permissions: Permissions,
    /// Providers on the protocols the core speaks, under names of their own.
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    /// MCP servers, the way to add tools; `./` in a command is the plugin's folder.
    #[serde(default)]
    pub mcp: BTreeMap<String, McpServerConfig>,
    #[serde(default)]
    pub hooks: Vec<HookConfig>,
    /// A program the core keeps running that brings the user in from elsewhere, such as
    /// a chat bot; it talks to the core over its stdin and stdout.
    pub platform: Option<Command>,
    /// A program that draws parts of the window with the host's widgets.
    pub ui: Option<Ui>,
}

/// The interface process and the parts it draws.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ui {
    pub command: Vec<String>,
    pub parts: Vec<PartConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartConfig {
    /// `[a-z0-9-]`, unique within the plugin.
    pub id: String,
    pub title: String,
    /// An SVG file in the plugin's folder.
    pub icon: Option<String>,
    pub place: Place,
    pub opens: Opens,
    /// Narrows the plugin's platforms; all of them when empty.
    #[serde(default)]
    pub platforms: Vec<Os>,
    /// The icon's text, once checked.
    #[serde(skip)]
    pub svg: Option<String>,
}

impl PartConfig {
    pub fn offered_here(&self) -> bool {
        runs_here(&self.platforms)
    }
}

/// A program run when `event` happens. It reads the event as JSON on stdin and may
/// answer on stdout; see [`super::hooks`].
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookConfig {
    pub event: HookEvent,
    /// For tool events, the tools it is run for; every tool when left out.
    #[serde(default)]
    pub tools: Vec<String>,
    pub command: Vec<String>,
    /// Seconds it may take; a hook that takes longer counts as failed.
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    TurnStart,
    UserMessage,
    BeforeTool,
    AfterTool,
    TurnEnd,
}

/// A command line: the program, then its arguments.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub command: Vec<String>,
}

fn default_timeout() -> u64 {
    10
}

impl Manifest {
    /// Reads `plugin.yml` in `folder`, whose name the plugin must carry.
    pub fn load(folder: &Path) -> Result<Self> {
        let file = folder.join(MANIFEST_FILE);
        let bytes = crate::init::files::read_regular(&file, MAX_MANIFEST_BYTES)
            .with_context(|| format!("cannot read {}", file.display()))?;
        let mut manifest: Self = serde_saphyr::from_slice(&bytes).context("invalid plugin.yml")?;
        let folder_name = folder.file_name().and_then(|name| name.to_str());
        ensure!(
            folder_name == Some(manifest.name.as_str()),
            "plugin.yml names {}, but its folder is {}",
            manifest.name,
            folder_name.unwrap_or("?")
        );
        ensure!(
            crate::init::paths::is_plain_folder_name(&manifest.name),
            "invalid plugin name {}",
            manifest.name
        );
        check_permissions(&manifest.permissions, &manifest.settings)?;
        for hook in &manifest.hooks {
            ensure!(!hook.command.is_empty(), "a hook has no command");
            ensure!(
                (1..=600).contains(&hook.timeout),
                "a hook's timeout is 1 to 600 seconds"
            );
        }
        if let Some(platform) = &manifest.platform {
            ensure!(!platform.command.is_empty(), "the platform has no command");
        }
        if let Some(ui) = &mut manifest.ui {
            check_ui(ui, folder, &manifest.platforms)?;
        }
        Ok(manifest)
    }

    /// The names of its keys, the settings that are secrets.
    pub fn secrets(&self) -> impl Iterator<Item = &String> {
        self.permissions.keys.iter().map(|key| &key.name)
    }

    /// Whether it is built for this system.
    pub fn runs_here(&self) -> bool {
        runs_here(&self.platforms)
    }

    /// Whether it runs programs of its own, which then run confined.
    pub fn runs_programs(&self) -> bool {
        !self.mcp.is_empty()
            || !self.hooks.is_empty()
            || self.platform.is_some()
            || self.ui.is_some()
    }
}

fn check_permissions(permissions: &Permissions, settings: &[String]) -> Result<()> {
    let mut origins = Vec::new();
    for origin in &permissions.network {
        origins.push(Origin::parse(origin)?);
    }
    for program in &permissions.exec {
        ensure!(
            !program.is_empty() && !program.contains(['/', '\\']) && program != "..",
            "exec names a program, such as xcrun, not a path: {program}"
        );
    }
    for folder in &permissions.read {
        let path = Path::new(folder.strip_prefix("~/").unwrap_or(folder));
        ensure!(
            (folder.starts_with("~/") || path.is_absolute())
                && !path.components().any(|part| part == Component::ParentDir),
            "read names a folder as /path or ~/path, without ..: {folder}"
        );
    }
    let mut names = BTreeSet::new();
    for key in &permissions.keys {
        ensure!(
            crate::init::paths::is_plain_folder_name(&key.name),
            "invalid key name {}",
            key.name
        );
        ensure!(
            names.insert(&key.name),
            "the key {} is named twice",
            key.name
        );
        ensure!(
            !settings.contains(&key.name),
            "a setting cannot also be a key: {}",
            key.name
        );
        ensure!(!key.origins.is_empty(), "the key {} goes nowhere", key.name);
        for origin in &key.origins {
            ensure!(
                origins.contains(&Origin::parse(origin)?),
                "the key {} goes to {origin}, which is not in network",
                key.name
            );
        }
    }
    Ok(())
}

fn check_ui(ui: &mut Ui, folder: &Path, platforms: &[Os]) -> Result<()> {
    ensure!(!ui.command.is_empty(), "the ui has no command");
    ensure!(!ui.parts.is_empty(), "the ui has no parts");
    let mut ids = BTreeSet::new();
    for part in &mut ui.parts {
        ensure!(
            is_part_id(&part.id),
            "a part's id is 1 to 40 of a-z, 0-9 and -: {}",
            part.id
        );
        ensure!(
            ids.insert(part.id.clone()),
            "the part {} is named twice",
            part.id
        );
        ensure!(
            !part.title.trim().is_empty() && part.title.chars().count() <= MAX_TITLE,
            "the part {} needs a title of up to {MAX_TITLE} characters",
            part.id
        );
        if let Some(os) = part.platforms.iter().find(|os| !allows(platforms, os)) {
            anyhow::bail!(
                "the part {} is for {}, which the plugin is not",
                part.id,
                os.name()
            );
        }
        if let Some(path) = &part.icon {
            let svg = icon::load(folder, path)
                .with_context(|| format!("the icon of part {}", part.id))?;
            part.svg = Some(svg);
        }
    }
    Ok(())
}

fn is_part_id(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(yaml: &str, files: &[(&str, &str)]) -> Result<Manifest> {
        let root = crate::testing::temp_dir("manifest");
        let folder = root.join("p");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(MANIFEST_FILE), format!("name: p\n{yaml}")).unwrap();
        for (name, text) in files {
            std::fs::write(folder.join(name), text).unwrap();
        }
        Manifest::load(&folder)
    }

    const ICON: &str = r#"<svg viewBox="0 0 1 1"><path d="M0 0h1v1z"/></svg>"#;

    fn ui(part: &str) -> String {
        format!("ui:\n  command: [./ui]\n  parts:\n{part}")
    }

    #[test]
    fn a_ui_part_loads_with_its_icon() {
        let part = "    - {id: sim-1, title: Simulator, icon: icon.svg, place: header, opens: right_panel, platforms: [macos]}\n";
        let manifest = load(&ui(part), &[("icon.svg", ICON)]).unwrap();
        let part = &manifest.ui.unwrap().parts[0];
        assert_eq!(part.svg.as_deref(), Some(ICON));
        assert_eq!(part.platforms, [Os::Macos]);
    }

    #[test]
    fn a_wrong_part_or_icon_refuses_the_manifest() {
        let part = |text: &str| ui(&format!("    - {{{text}, place: header, opens: window}}\n"));
        for (yaml, why) in [
            (part("id: Big, title: T"), "id"),
            (part("id: '', title: T"), "id"),
            (part(&format!("id: {}, title: T", "a".repeat(41))), "id"),
            (part("id: a, title: ' '"), "title"),
            (part("id: a, title: T, icon: ../icon.svg"), "icon outside"),
            (part("id: a, title: T, icon: bad.svg"), "icon with a script"),
            (part("id: a, title: T, icon: none.svg"), "missing icon"),
            (part("id: a, title: T, color: red"), "unknown field"),
            (part("id: a, title: T, platforms: [beos]"), "platform"),
            (
                format!(
                    "platforms: [linux]\n{}",
                    part("id: a, title: T, platforms: [macos]")
                ),
                "part outside the plugin's platforms",
            ),
            (
                ui(
                    "    - {id: a, title: T, place: header, opens: window}\n    - {id: a, title: U, place: rail, opens: window}\n",
                ),
                "same id",
            ),
            ("ui:\n  command: []\n  parts: []\n".to_owned(), "no command"),
        ] {
            let files = [("icon.svg", ICON), ("bad.svg", "<svg><script/></svg>")];
            assert!(load(&yaml, &files).is_err(), "{why}");
        }
    }

    #[test]
    fn platforms_and_clients_are_read() {
        let manifest = load("platforms: [macos, windows]\nclients: [gui, web]\n", &[]).unwrap();
        assert_eq!(manifest.platforms, [Os::Macos, Os::Windows]);
        assert_eq!(manifest.clients, [Client::Gui, Client::Web]);
        assert!(load("clients: [phone]\n", &[]).is_err());
        assert!(load("platforms: macos\n", &[]).is_err());
    }

    #[test]
    fn permissions_are_checked() {
        let good = "permissions:\n  network: [https://api.example.com]\n  \
                    keys: [{name: token, origins: [https://api.example.com]}]\n";
        assert!(load(good, &[]).is_ok());
        for bad in [
            "permissions:\n  network: [http://api.example.com]\n",
            "permissions:\n  network: [https://api.example.com/path]\n",
            "permissions:\n  network: [https://a.example.com]\n  keys: [{name: token, origins: [https://b.example.com]}]\n",
            "permissions:\n  network: [https://a.example.com]\n  keys: [{name: token, origins: []}]\n",
            "permissions:\n  exec: [/bin/sh]\n",
            "permissions:\n  read: [relative/folder]\n",
            "permissions:\n  read: [~/../etc]\n",
            "secrets: [token]\n",
            "settings: [token]\npermissions:\n  network: [https://a.example.com]\n  keys: [{name: token, origins: [https://a.example.com]}]\n",
        ] {
            assert!(load(bad, &[]).is_err(), "{bad}");
        }
    }

    #[test]
    fn shipped_plugins_load() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/plugins");
        for entry in std::fs::read_dir(root).unwrap() {
            let folder = entry.unwrap().path();
            if !folder.is_dir() {
                continue;
            }
            Manifest::load(&folder)
                .unwrap_or_else(|error| panic!("{}: {error:#}", folder.display()));
        }
    }
}
