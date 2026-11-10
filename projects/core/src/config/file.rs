//! The config file itself: what it holds, how it is read privately, checked and
//! written back.

use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io::Read,
    path::Path,
};

use anyhow::{Context, Result, anyhow, ensure};
use serde::{Deserialize, Serialize};

use crate::{
    catalog::modes::{self, Modes},
    release::update::Channel,
    tools::ToolKind,
};

use super::sections::{is_zero, yes};
use super::{
    Appearance, ApprovalPolicy, ContextPolicy, Effort, Limits, Logging, McpServerConfig, Memory,
    Plugged, PluginConfig, ProviderConfig, ThreadConfig, VersionControl, WorkHistory,
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
    /// Plugins by name: whether each is on, and its settings.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    plugins: BTreeMap<String, PluginConfig>,
    /// What the plugins that are on add; never saved.
    #[serde(skip)]
    plugged: Plugged,
}

impl Config {
    /// Reads a private local file: neither the file nor its directory may be a symlink or
    /// readable by group or others.
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = read_private_file(path.as_ref())?;
        Self::parse(&bytes)
    }

    /// The providers of the file; [`Config::all_providers`] adds the plugins' ones.
    pub fn providers(&self) -> &BTreeMap<String, ProviderConfig> {
        &self.providers
    }

    /// The MCP servers of the file; [`Config::all_mcp`] adds the plugins' ones.
    pub fn mcp(&self) -> &BTreeMap<String, McpServerConfig> {
        &self.mcp
    }

    /// The file's providers and those of the plugins that are on.
    pub fn all_providers(&self) -> BTreeMap<String, ProviderConfig> {
        let mut all = self.plugged.providers.clone();
        all.extend(self.providers.clone());
        all
    }

    /// The file's MCP servers and those of the plugins that are on.
    pub fn all_mcp(&self) -> BTreeMap<String, McpServerConfig> {
        let mut all = self.plugged.mcp.clone();
        all.extend(self.mcp.clone());
        all
    }

    pub fn plugins(&self) -> &BTreeMap<String, PluginConfig> {
        &self.plugins
    }

    pub fn plugin(&self, name: &str) -> PluginConfig {
        self.plugins.get(name).cloned().unwrap_or_default()
    }

    pub fn set_plugin_enabled(&mut self, name: &str, enabled: bool) {
        self.plugins.entry(name.to_owned()).or_default().enabled = enabled;
    }

    pub fn set_plugged(&mut self, plugged: Plugged) {
        self.plugged = plugged;
    }

    pub fn plugged(&self) -> &Plugged {
        &self.plugged
    }

    /// Adds, replaces or, with `None`, removes an MCP server.
    pub fn set_mcp_server(&mut self, name: &str, server: Option<McpServerConfig>) {
        match server {
            Some(server) => self.mcp.insert(name.to_owned(), server),
            None => self.mcp.remove(name),
        };
    }

    pub fn set_mcp_enabled(&mut self, name: &str, enabled: bool) {
        if let Some(server) = self.mcp.get_mut(name) {
            server.set_enabled(enabled);
        }
    }

    pub fn thread(&self) -> &ThreadConfig {
        &self.thread
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    pub fn sound_notifications(&self) -> bool {
        self.sound_notifications
    }

    /// Whether clients notify at all.
    pub fn notifications(&self) -> bool {
        self.notifications
    }

    pub fn set_notifications(&mut self, on: bool) {
        self.notifications = on;
    }

    pub fn set_sound_notifications(&mut self, on: bool) {
        self.sound_notifications = on;
    }

    pub fn version_control(&self) -> &VersionControl {
        &self.version_control
    }

    pub fn set_branch_prefix(&mut self, prefix: &str) {
        self.version_control.branch_prefix = prefix.trim().to_owned();
    }

    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    pub fn set_idle_screen_minutes(&mut self, minutes: u32) {
        self.appearance.idle_screen_minutes = minutes.min(Appearance::MAX_IDLE_SCREEN_MINUTES);
    }

    /// Whether the file switches the tool on, whatever the mode; the tests read it back.
    #[cfg(test)]
    pub fn tool_enabled(&self, name: &str) -> bool {
        self.tools.get(name).copied().unwrap_or(true)
    }

    pub fn set_tool_enabled(&mut self, name: &str, enabled: bool) {
        self.tools.insert(name.to_owned(), enabled);
    }

    pub fn mode(&self) -> &str {
        self.mode.as_deref().unwrap_or(modes::MAIN)
    }

    pub fn set_mode(&mut self, mode: String) {
        self.mode = (mode != modes::MAIN).then_some(mode);
    }

    /// A tool's switch in a mode: the user's, else the mode's default, else MAIN's.
    pub fn mode_tool_enabled(&self, mode: &str, name: &str, kind: ToolKind) -> bool {
        let main = || {
            self.tools
                .get(name)
                .copied()
                .unwrap_or(kind.on_by_default())
        };
        if mode == modes::MAIN {
            return main();
        }
        let switched = self.modes.get(mode).and_then(|tools| tools.get(name));
        switched
            .copied()
            .or_else(|| Modes::default_tool(mode, name, kind))
            .unwrap_or_else(main)
    }

    pub fn set_mode_tool_enabled(&mut self, mode: &str, name: &str, enabled: bool) {
        if mode == modes::MAIN {
            return self.set_tool_enabled(name, enabled);
        }
        let tools = self.modes.entry(mode.to_owned()).or_default();
        tools.insert(name.to_owned(), enabled);
    }

    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    pub fn set_model(&mut self, name: String) {
        self.model = Some(name);
    }

    pub fn memory(&self) -> Memory {
        self.memory
    }

    pub fn set_memory_enabled(&mut self, enabled: bool) {
        self.memory.enabled = enabled;
    }

    pub fn set_memory_chars(&mut self, chars: usize) {
        self.memory.max_chars = chars;
    }

    pub fn work_history(&self) -> WorkHistory {
        self.work_history
    }

    pub fn set_work_history_enabled(&mut self, enabled: bool) {
        self.work_history.enabled = enabled;
    }

    pub fn set_work_history_chars(&mut self, chars: usize) {
        self.work_history.max_chars = chars;
    }

    pub fn update_channel(&self) -> Channel {
        self.update_channel.unwrap_or_default()
    }

    pub fn set_update_channel(&mut self, channel: Channel) {
        self.update_channel = (channel != Channel::Stable).then_some(channel);
    }

    pub fn approvals(&self) -> ApprovalPolicy {
        self.approvals
    }

    pub fn logging(&self) -> &Logging {
        &self.logging
    }

    pub fn set_logging_enabled(&mut self, enabled: bool) {
        self.logging.enabled = enabled;
    }

    pub fn set_approvals(&mut self, policy: ApprovalPolicy) {
        self.approvals = policy;
    }

    pub fn effort(&self) -> Option<Effort> {
        self.effort
    }

    pub fn set_effort(&mut self, effort: Option<Effort>) {
        if self.effort == effort {
            return;
        }
        self.previous_effort = if effort == Some(Effort::Off) {
            self.effort
        } else {
            None
        };
        self.effort = effort;
        self.effort_revision = self.effort_revision.wrapping_add(1);
    }

    /// A delayed rejection must not overwrite an effort selected after its request.
    pub(crate) fn restore_unsupported_effort(&mut self, requested: &Config) -> bool {
        if requested.effort != Some(Effort::Off)
            || self.effort != Some(Effort::Off)
            || self.effort_revision != requested.effort_revision
        {
            return false;
        }
        self.set_effort(self.previous_effort);
        true
    }

    pub fn context_policy(&self, provider: &str, model: &str) -> ContextPolicy {
        self.model_contexts
            .get(&format!("{provider}/{model}"))
            .copied()
            .unwrap_or_default()
    }

    pub fn set_context_policy(&mut self, provider: &str, model: &str, policy: ContextPolicy) {
        let name = format!("{provider}/{model}");
        if policy == ContextPolicy::Auto {
            self.model_contexts.remove(&name);
        } else {
            self.model_contexts.insert(name, policy);
        }
    }

    pub fn input_limit(&self, provider: &str, model: &str, window: Option<u64>) -> u64 {
        self.context_policy(provider, model).input_limit(window)
    }

    pub fn set_provider(&mut self, name: String, provider: ProviderConfig) {
        self.providers.insert(name, provider);
    }

    pub fn set_max_agents_at_work(&mut self, limit: usize) {
        self.limits.max_agents_at_work = limit;
    }

    pub fn set_max_goal_rounds(&mut self, rounds: u32) {
        self.limits.max_goal_rounds = rounds;
    }

    pub fn agent_enabled(&self, name: &str) -> bool {
        self.agents.get(name).copied().unwrap_or(true)
    }

    pub fn set_agent_enabled(&mut self, name: &str, enabled: bool) {
        self.agents.insert(name.to_owned(), enabled);
    }

    pub fn skill_enabled(&self, name: &str) -> bool {
        self.skills.get(name).copied().unwrap_or(true)
    }

    pub fn set_skill_enabled(&mut self, name: &str, enabled: bool) {
        self.skills.insert(name.to_owned(), enabled);
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

    fn validate(&self) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{UNKNOWN_WINDOW, mcp_server_yaml, parse_mcp_server};

    #[test]
    fn auto_takes_the_model_window() {
        let auto = ContextPolicy::Auto;
        assert_eq!(auto.input_limit(Some(256_000)), 256_000);
        assert_eq!(auto.input_limit(None), UNKNOWN_WINDOW);
        let manual = ContextPolicy::Manual(300_000);
        assert_eq!(manual.input_limit(Some(256_000)), 256_000);
        assert_eq!(manual.input_limit(None), 300_000);
    }

    const PROVIDERS: &str = "providers:\n  main:\n    kind: openai_compatible\n    base_url: 'https://host/v1'\n    api_key: 'sk-test'\n";

    fn parse(yaml: &str) -> Result<Config> {
        Config::parse(yaml.as_bytes())
    }

    #[test]
    fn unknown_fields_and_values_are_rejected() {
        assert!(parse(&format!("{PROVIDERS}session:\n  record: everything\n")).is_err());
        assert!(parse(&format!("{PROVIDERS}tool: {{}}\n")).is_err());
        assert!(parse("providers: {}\n").is_ok(), "offline config");
    }

    #[test]
    fn an_mcp_server_is_edited_as_its_own_yaml() {
        let (name, mut server) =
            parse_mcp_server("docs:\n  command: docs-server\n  args: [--stdio]\n").unwrap();
        assert_eq!((name.as_str(), server.enabled()), ("docs", true));
        server.set_enabled(false);
        let text = mcp_server_yaml(&name, &server).unwrap();
        let (_, again) = parse_mcp_server(&text).unwrap();
        assert!(!again.enabled() && again.target() == "docs-server --stdio");
        assert!(parse_mcp_server("a:\n  command: x\nb:\n  command: y\n").is_err());
        assert!(parse_mcp_server("docs:\n  command: x\n  extra: 1\n").is_err());
    }

    #[test]
    fn traces_go_encrypted_or_stay_on_this_machine() {
        let traces = |endpoint: &str| {
            parse(&format!(
                "logging:\n  traces:\n    endpoint: '{endpoint}'\n"
            ))
        };
        assert!(traces("https://otel.example.com").is_ok());
        assert!(traces("http://127.0.0.1:4318").is_ok());
        for wrong in [
            "http://otel.example.com",
            "https://k:s@otel.example.com",
            "otel",
        ] {
            assert!(traces(wrong).is_err(), "{wrong}");
        }
        let off = parse("logging:\n  enabled: false\n").unwrap();
        assert!(!off.logging().enabled);
    }

    #[test]
    fn mcp_servers_are_one_kind_and_reached_safely() {
        let server = |body: &str| parse(&format!("mcp:\n  docs:\n{body}"));
        assert!(server("    command: docs-server\n").is_ok());
        assert!(server("    url: 'https://mcp.example.com/mcp'\n").is_ok());
        assert!(server("    url: 'http://127.0.0.1:9000/mcp'\n").is_ok());
        for wrong in [
            "    command: docs-server\n    url: 'https://mcp.example.com/mcp'\n",
            "    url: 'http://mcp.example.com/mcp'\n",
            "    url: 'https://user:pass@mcp.example.com/mcp'\n",
            "    url: 'https://mcp.example.com/mcp'\n    headers:\n      'bad name': x\n",
        ] {
            assert!(server(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn written_yaml_reads_back_the_same() {
        let mut config = parse(PROVIDERS).unwrap();
        config.set_tool_enabled("run_command", false);
        config.set_branch_prefix("dusk/");
        config.set_notifications(false);
        config.set_sound_notifications(false);
        let yaml = config.to_yaml().unwrap();
        assert!(yaml.starts_with(HEADER));
        let again = parse(&yaml).unwrap();
        assert!(!again.tool_enabled("run_command"));
        assert_eq!(again.providers()["main"].api_key.0, "sk-test");
        assert_eq!(again.version_control().branch_prefix, "dusk/");
        assert!(!again.notifications());
        assert!(!again.sound_notifications());
    }

    #[test]
    fn branch_prefixes_are_bounded_and_safe() {
        assert!(
            parse(&format!(
                "{PROVIDERS}version_control:\n  branch_prefix: 'dusk/'\n"
            ))
            .is_ok()
        );
        assert!(parse("version_control:\n  branch_prefix: ''\n").is_ok());
        for bad in ["a//b", "..", "a b/", "/a", &"x".repeat(65)] {
            assert!(
                parse(&format!("version_control:\n  branch_prefix: '{bad}'\n")).is_err(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn notifications_default_to_on_and_stay_off_when_set() {
        let plain = parse(PROVIDERS).unwrap();
        assert!(plain.notifications() && plain.sound_notifications());
        let off = parse("notifications: false\n").unwrap();
        assert!(!off.notifications() && off.sound_notifications());
        let silent = parse("sound_notifications: false\n").unwrap();
        assert!(silent.notifications() && !silent.sound_notifications());
    }
}
