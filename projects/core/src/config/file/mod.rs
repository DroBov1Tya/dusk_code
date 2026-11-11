//! The config file itself: what it holds, how it is read privately, checked and
//! written back.

mod model;
mod preferences;
mod providers;
mod switches;
#[cfg(test)]
mod tests;

use super::sections::{is_zero, yes};
use super::{
    Appearance, ApprovalPolicy, ContextPolicy, Effort, Limits, Logging, McpServerConfig, Memory,
    Plugged, PluginConfig, ProviderConfig, ThreadConfig, VersionControl, WorkHistory,
};
use crate::release::update::Channel;
use anyhow::{Context, Result, anyhow, ensure};
use dusk_protocol::config::VoiceSettings;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io::Read,
    path::Path,
};

/// MCP server names are this long at most.
const MAX_MCP_NAME: usize = 24;

/// Branch prefixes are this long at most.
const MAX_BRANCH_PREFIX: usize = 64;

/// Written above the settings whenever the core saves the file.
const HEADER: &str = "\
# dusk_code settings. The core rewrites this file when settings change at runtime,
# so comments other than this header are not kept. Keep it private (mode 0600).
";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    providers: BTreeMap<String, ProviderConfig>,
    /// Called `session` in older files.
    #[serde(default, alias = "session")]
    thread: ThreadConfig,
    #[serde(default)]
    limits: Limits,
    /// Ring the terminal bell when the visible thread completes or fails. A terminal
    /// profile may silence the bell; set this to false to disable notifications here.
    #[serde(default = "yes")]
    sound_notifications: bool,
    /// Whether clients notify at all; off keeps them silent whatever sound says.
    #[serde(default = "yes")]
    notifications: bool,
    /// Version control settings; clients show this section as "Version control".
    #[serde(default, skip_serializing_if = "VersionControl::is_default")]
    version_control: VersionControl,
    #[serde(default, skip_serializing_if = "Appearance::is_default")]
    appearance: Appearance,
    /// Tools switched on or off by name; a tool not listed is on.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    tools: BTreeMap<String, bool>,
    /// Skills switched on or off by name; a skill not listed is on.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    skills: BTreeMap<String, bool>,
    /// Agents switched on or off by name; an agent not listed is on.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    agents: BTreeMap<String, bool>,
    /// The model threads talk to, as `provider/model`; the first model of the first
    /// provider when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    /// The chat's mode, by its file in `modes/`; MAIN when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    /// Tool switches of the modes other than MAIN, which uses `tools`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    modes: BTreeMap<String, BTreeMap<String, bool>>,
    /// What the agent remembers about the user, in MEMORY.md.
    #[serde(default, skip_serializing_if = "Memory::is_default")]
    memory: Memory,
    /// What the agent remembers about the project in the working directory.
    #[serde(default, skip_serializing_if = "WorkHistory::is_default")]
    work_history: WorkHistory,
    /// Which releases /update offers; stable when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    update_channel: Option<Channel>,
    /// How hard the model reasons; the provider's default when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    effort: Option<Effort>,
    /// Effort before Off was selected, so an explicit rejection can restore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_effort: Option<Effort>,
    /// Fences a rejection from a request started before a later settings change.
    #[serde(default, skip_serializing_if = "is_zero")]
    effort_revision: u64,
    /// Input policies keyed by the stable `provider/model` name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    model_contexts: BTreeMap<String, ContextPolicy>,
    /// MCP servers by name, started with the core.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    mcp: BTreeMap<String, McpServerConfig>,
    /// Whether tools ask before a path outside their folders and before every command.
    #[serde(default, skip_serializing_if = "ApprovalPolicy::is_ask")]
    approvals: ApprovalPolicy,
    /// Log files, and traces for an OpenTelemetry collector.
    #[serde(default, skip_serializing_if = "Logging::is_default")]
    logging: Logging,
    /// The speech servers: where recordings become text and text becomes sound.
    #[serde(default, skip_serializing_if = "VoiceSettings::is_default")]
    voice: VoiceSettings,
    /// Plugins by name: whether each is on, and its settings.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    plugins: BTreeMap<String, PluginConfig>,
    /// What the plugins that are on add; never saved.
    #[serde(skip)]
    plugged: Plugged,
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("providers", &"[REDACTED]")
            .field("provider_count", &self.providers.len())
            .field("thread", &self.thread)
            .field("limits", &self.limits)
            .field("tools", &self.tools)
            .field("skills", &self.skills)
            .field("agents", &self.agents)
            .field("model", &self.model)
            .field("mcp", &self.mcp.keys().collect::<Vec<_>>())
            .field("logging", &self.logging.enabled)
            .finish()
    }
}

/// Whether a branch prefix can start a branch name: path components of letters, digits,
/// `-`, `_` and `.` separated by single `/`, such as `dusk/`. A prefix is the part this
/// tool puts in front, so it may end with `/`; Git's full rules apply to the whole name.
fn valid_branch_prefix(prefix: &str) -> bool {
    !prefix.starts_with('/')
        && !prefix.contains("//")
        && !prefix.contains("..")
        && prefix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
}

fn read_private_file(path: &Path) -> Result<Vec<u8>> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("config directory is missing"))?;
    let directory =
        fs::symlink_metadata(parent).map_err(|_| anyhow!("cannot inspect config directory"))?;
    ensure!(
        directory.is_dir(),
        "config directory must be a directory, not a symlink"
    );
    ensure_private(&directory)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| anyhow!("cannot inspect config file"))?;
    ensure!(
        metadata.is_file(),
        "config must be a regular file, not a symlink"
    );
    ensure_private(&metadata)?;
    let file = File::open(path).map_err(|_| anyhow!("cannot open config file"))?;
    let metadata = file
        .metadata()
        .map_err(|_| anyhow!("cannot inspect open config file"))?;
    ensure!(metadata.is_file(), "config must be a regular file");
    ensure_private(&metadata)?;
    let mut bytes = Vec::new();
    (&file)
        .read_to_end(&mut bytes)
        .map_err(|_| anyhow!("cannot read config file"))?;
    Ok(bytes)
}

fn ensure_private(metadata: &fs::Metadata) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            metadata.permissions().mode() & 0o077 == 0,
            "config directory and file must not be accessible by group or others"
        );
    }
    #[cfg(not(unix))]
    let _ = metadata;
    Ok(())
}

impl Config {
    /// Reads a private local file: neither the file nor its directory may be a symlink or
    /// readable by group or others.
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = read_private_file(path.as_ref())?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        // YAML errors can quote input values, including secrets. Drop their source.
        let config: Self =
            serde_saphyr::from_slice(bytes).map_err(|_| anyhow!("invalid configuration YAML"))?;
        config.validate()?;
        Ok(config)
    }

    pub fn to_yaml(&self) -> Result<String> {
        let body = serde_saphyr::to_string(self).context("cannot encode configuration")?;
        Ok(format!("{HEADER}{body}"))
    }

    pub(super) fn validate(&self) -> Result<()> {
        for endpoint in [&self.voice.stt, &self.voice.tts].into_iter().flatten() {
            crate::voice::check_endpoint(endpoint)?;
        }
        ensure!(
            self.limits.max_agents_at_work > 0,
            "max_agents_at_work must be above zero"
        );
        let prefix = &self.version_control.branch_prefix;
        ensure!(
            prefix.len() <= MAX_BRANCH_PREFIX,
            "version_control.branch_prefix must be at most {MAX_BRANCH_PREFIX} characters"
        );
        ensure!(
            prefix.is_empty() || valid_branch_prefix(prefix),
            "version_control.branch_prefix is not a valid branch prefix"
        );
        ensure!(
            self.memory.max_chars > 0,
            "memory.max_chars must be above zero"
        );
        ensure!(
            self.work_history.max_chars > 0,
            "work_history.max_chars must be above zero"
        );
        ensure!(
            self.limits.max_goal_rounds > 0,
            "max_goal_rounds must be above zero"
        );
        ensure!(
            self.appearance.idle_screen_minutes <= Appearance::MAX_IDLE_SCREEN_MINUTES,
            "appearance.idle_screen_minutes must be at most {}",
            Appearance::MAX_IDLE_SCREEN_MINUTES
        );
        ensure!(
            self.previous_effort != Some(Effort::Off),
            "previous effort cannot be off"
        );
        if let Some(traces) = &self.logging.traces {
            // Spans name files and commands; they travel encrypted or stay on this machine.
            let url = reqwest::Url::parse(&traces.endpoint)
                .map_err(|_| anyhow!("logging.traces.endpoint is not a URL"))?;
            ensure!(
                url.scheme() == "https" && url.host_str().is_some()
                    || crate::providers::is_loopback(&url),
                "logging.traces.endpoint must use HTTPS, or HTTP to 127.0.0.1 or [::1]"
            );
            ensure!(
                url.username().is_empty() && url.password().is_none() && url.query().is_none(),
                "logging.traces.endpoint must not contain credentials or a query"
            );
        }
        for (name, policy) in &self.model_contexts {
            let valid = name.split_once('/').is_some_and(|(provider, model)| {
                !provider.trim().is_empty() && !model.trim().is_empty()
            });
            ensure!(
                valid && !name.chars().any(char::is_control),
                "invalid model context key"
            );
            policy.validate()?;
        }
        for (name, provider) in &self.providers {
            ensure!(!name.trim().is_empty(), "provider name must not be empty");
            ensure!(
                !provider.base_url.trim().is_empty(),
                "provider base URL must not be empty"
            );
            ensure!(
                provider.api_key.0.is_empty()
                    || (!provider.api_key.0.trim().is_empty()
                        && !provider.api_key.0.chars().any(char::is_control)),
                "provider API key is invalid"
            );
        }
        for (name, server) in &self.mcp {
            // The name becomes part of tool names, which models accept only in this shape.
            ensure!(
                !name.is_empty()
                    && name.len() <= MAX_MCP_NAME
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "MCP server name {name:?} must be up to {MAX_MCP_NAME} of a-z, 0-9 and -"
            );
            match server {
                McpServerConfig::Stdio(server) => ensure!(
                    !server.command.trim().is_empty(),
                    "MCP server {name} has no command"
                ),
                McpServerConfig::Http(server) => {
                    crate::mcp::check_url(&server.url)
                        .with_context(|| format!("MCP server {name}"))?;
                    crate::mcp::check_headers(server)
                        .with_context(|| format!("MCP server {name}"))?;
                }
            }
        }
        Ok(())
    }
}
