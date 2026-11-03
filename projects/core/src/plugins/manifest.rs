//! `plugin.yml`, what a plugin says it brings. Unknown fields are refused, so a typo
//! shows up as an error instead of a part that silently does nothing.

use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::config::{McpServerConfig, ProviderConfig};

pub const MANIFEST_FILE: &str = "plugin.yml";
/// The first word of a command that stands for this duskcode binary.
pub const SELF_COMMAND: &str = "duskcode";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The same as the plugin's folder.
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// Settings the user gives it under `plugins:` in the config, such as a token.
    #[serde(default)]
    pub settings: Vec<String>,
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
        let bytes = crate::util::fs::read_regular(&file, MAX_MANIFEST_BYTES)
            .with_context(|| format!("cannot read {}", file.display()))?;
        let manifest: Self = serde_saphyr::from_slice(&bytes).context("invalid plugin.yml")?;
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
        Ok(manifest)
    }
}
