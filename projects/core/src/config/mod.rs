//! The settings in `config.yml`: providers, MCP servers, what threads record and which
//! tools and skills are on.
//! `LiveConfig` keeps the current copy in memory and writes changes back.

pub mod live;

use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io::Read,
    path::Path,
};

use anyhow::{Context, Result, anyhow, ensure};
use serde::{Deserialize, Serialize};

use crate::{catalog::modes, release::update::Channel, tools::ToolKind};

/// MCP server names are this long at most.
const MAX_MCP_NAME: usize = 24;

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
}

/// Ask is the default; full access answers every request with yes. The config folder
/// stays closed either way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPolicy {
    #[default]
    Ask,
    FullAccess,
}

impl ApprovalPolicy {
    fn is_ask(&self) -> bool {
        *self == Self::Ask
    }
}

/// The memory of the user: whether the agent keeps it, and its size. Every request
/// carries it, so it is kept small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Memory {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_memory_chars")]
    pub max_chars: usize,
}

impl Default for Memory {
    fn default() -> Self {
        Self {
            enabled: true,
            max_chars: default_memory_chars(),
        }
    }
}

impl Memory {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn default_memory_chars() -> usize {
    2000
}

/// Reasoning effort. Off is explicit; the provider default omits `reasoning_effort`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Off,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    Ultra,
}

impl Effort {
    pub const ALL: [Self; 8] = [
        Self::Off,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Xhigh,
        Self::Max,
        Self::Ultra,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
            Self::Ultra => "ultra",
        }
    }

    /// OpenAI-compatible APIs call explicit Off `none`.
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Off => "none",
            other => other.name(),
        }
    }

    /// `default` omits the parameter: the provider decides.
    pub fn parse(text: &str) -> Result<Option<Self>> {
        if text == "default" {
            return Ok(None);
        }
        Self::ALL
            .into_iter()
            .find(|effort| effort.name() == text)
            .map(Some)
            .with_context(|| {
                format!(
                    "unknown effort {text}; off, minimal, low, medium, high, xhigh, max, ultra or default"
                )
            })
    }
}

/// A model's input budget. Auto is the model's own window as its provider reports it,
/// and the global input cap for a model that reports none. Fixed and Manual are set by
/// hand; a known window stays their ceiling.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPolicy {
    #[default]
    Auto,
    Fixed(ContextPreset),
    Manual(u64),
}

/// Decimal token counts, matching model catalogue context windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextPreset {
    #[serde(rename = "128k")]
    K128,
    #[serde(rename = "256k")]
    K256,
    #[serde(rename = "512k")]
    K512,
    #[serde(rename = "1m")]
    M1,
}

impl ContextPreset {
    pub const ALL: [Self; 4] = [Self::K128, Self::K256, Self::K512, Self::M1];

    pub fn tokens(self) -> u64 {
        match self {
            Self::K128 => 128_000,
            Self::K256 => 256_000,
            Self::K512 => 512_000,
            Self::M1 => 1_000_000,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::K128 => "128k",
            Self::K256 => "256k",
            Self::K512 => "512k",
            Self::M1 => "1M",
        }
    }
}

impl ContextPolicy {
    pub fn validate(self) -> Result<()> {
        ensure!(
            !matches!(self, Self::Manual(0)),
            "manual context must be above zero"
        );
        Ok(())
    }

    /// The single resolver used by request guards, context reports and compaction.
    pub fn input_limit(self, global: u64, model_window: Option<u64>) -> u64 {
        let selected = match self {
            Self::Auto => return model_window.unwrap_or(global),
            Self::Fixed(preset) => preset.tokens(),
            Self::Manual(tokens) => tokens,
        };
        model_window.map_or(selected, |window| window.min(selected))
    }
}

/// An MCP server: a `command` the core starts, or a `url` it reaches.
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpServerConfig {
    Stdio(StdioServerConfig),
    Http(HttpServerConfig),
}

/// A server the core starts as a child process and talks to over stdio. `env` may hold
/// secrets, so it is never logged.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StdioServerConfig {
    pub command: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// A remote server over Streamable HTTP. `headers` go with every request and may hold
/// secrets, such as an authorization, so they are never logged.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpServerConfig {
    pub url: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

impl McpServerConfig {
    pub fn enabled(&self) -> bool {
        match self {
            Self::Stdio(server) => server.enabled,
            Self::Http(server) => server.enabled,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        match self {
            Self::Stdio(server) => server.enabled = enabled,
            Self::Http(server) => server.enabled = enabled,
        }
    }

    /// The command line or the URL, for a client to show.
    pub fn target(&self) -> String {
        match self {
            Self::Stdio(server) => std::iter::once(server.command.as_str())
                .chain(server.args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" "),
            Self::Http(server) => server.url.clone(),
        }
    }
}

/// One MCP server as YAML, `name:` with its fields below, for editing by hand.
pub fn mcp_server_yaml(name: &str, server: &McpServerConfig) -> Result<String> {
    let one = BTreeMap::from([(name, server)]);
    serde_saphyr::to_string(&one).context("cannot encode the MCP server")
}

/// One MCP server from the YAML `mcp_server_yaml` writes. It is checked with the rest
/// of the config when it is saved.
pub fn parse_mcp_server(text: &str) -> Result<(String, McpServerConfig)> {
    // YAML errors can quote values, and headers or env may hold secrets.
    let servers: BTreeMap<String, McpServerConfig> =
        serde_saphyr::from_str(text).map_err(|_| {
            anyhow!("expected `name:` and below it `command:` or `url:` with their fields")
        })?;
    let mut servers = servers.into_iter();
    match (servers.next(), servers.next()) {
        (Some(server), None) => Ok(server),
        _ => Err(anyhow!("write exactly one server")),
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    #[serde(default)]
    pub kind: ProviderKind,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "ApiKey::is_empty")]
    pub api_key: ApiKey,
    /// The server leaves `<think>` and `<tool_call>` tags inside the answer text.
    #[serde(default, skip_serializing_if = "is_false")]
    pub inline_tags: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProviderKind {
    #[default]
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
    /// OpenAI itself: Chat Completions with `max_completion_tokens`.
    #[serde(rename = "openai")]
    OpenAi,
    /// The Anthropic Messages API.
    #[serde(rename = "anthropic")]
    Anthropic,
}

impl ProviderKind {
    pub const ALL: [Self; 3] = [Self::OpenAiCompatible, Self::Anthropic, Self::OpenAi];

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "Custom (OpenAI-compatible)",
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
        }
    }

    /// The provider's own API, for a form to fill in; none for a custom one.
    pub fn base_url(self) -> Option<&'static str> {
        match self {
            Self::OpenAiCompatible => None,
            Self::OpenAi => Some("https://api.openai.com/v1"),
            Self::Anthropic => Some("https://api.anthropic.com/v1"),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApiKey(pub String);

impl ApiKey {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThreadConfig {
    #[serde(default)]
    pub record: RecordMode,
}

/// Token limits for every model call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// For a model whose window is not known, a request larger than this is not sent;
    /// the context is compacted well before it.
    #[serde(default = "default_token_limit")]
    pub max_input_tokens: u64,
    /// The model is stopped after writing this many tokens in one call.
    #[serde(default = "default_token_limit")]
    pub max_output_tokens: u64,
    /// Maximum number of subagents running across all threads.
    #[serde(default = "default_max_agents_at_work")]
    pub max_agents_at_work: usize,
    /// Turns a goal may start by itself before it is paused for the user.
    #[serde(default = "default_max_goal_rounds")]
    pub max_goal_rounds: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_tokens: default_token_limit(),
            max_output_tokens: default_token_limit(),
            max_agents_at_work: default_max_agents_at_work(),
            max_goal_rounds: default_max_goal_rounds(),
        }
    }
}

fn default_token_limit() -> u64 {
    100_000
}

fn default_max_agents_at_work() -> usize {
    8
}

fn default_max_goal_rounds() -> u32 {
    10
}

/// What the core records about its work. Off, nothing is written or sent at all.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logging {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Sends spans of turns, model calls and tool calls over OTLP/HTTP.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traces: Option<Traces>,
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            enabled: true,
            traces: None,
        }
    }
}

impl Logging {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// An OpenTelemetry collector. `headers` may hold an API key, so they are never logged.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Traces {
    /// The collector's OTLP/HTTP address, such as `http://127.0.0.1:4318`; spans go to
    /// `/v1/traces` under it.
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

/// How the client looks and behaves on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Appearance {
    /// Minutes with no input and no work before the idle screen plays; 0 never.
    #[serde(default = "default_idle_screen_minutes")]
    pub idle_screen_minutes: u32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            idle_screen_minutes: default_idle_screen_minutes(),
        }
    }
}

impl Appearance {
    /// A day is past any idle a user would wait for.
    pub const MAX_IDLE_SCREEN_MINUTES: u32 = 24 * 60;

    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn default_idle_screen_minutes() -> u32 {
    5
}

/// What a thread stores from the model output. User messages are always stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RecordMode {
    /// Everything the model produces: reasoning, tool calls and the answer.
    #[default]
    Full,
    /// Only the answer.
    Answer,
}

impl Config {
    /// Reads a private local file: neither the file nor its directory may be a symlink or
    /// readable by group or others.
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = read_private_file(path.as_ref())?;
        Self::parse(&bytes)
    }

    pub fn providers(&self) -> &BTreeMap<String, ProviderConfig> {
        &self.providers
    }

    pub fn mcp(&self) -> &BTreeMap<String, McpServerConfig> {
        &self.mcp
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

    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    pub fn set_idle_screen_minutes(&mut self, minutes: u32) {
        self.appearance.idle_screen_minutes = minutes.min(Appearance::MAX_IDLE_SCREEN_MINUTES);
    }

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
        if mode == modes::MAIN {
            return self.tool_enabled(name);
        }
        let switched = self.modes.get(mode).and_then(|tools| tools.get(name));
        switched
            .copied()
            .or_else(|| modes::default_tool(mode, name, kind))
            .unwrap_or_else(|| self.tool_enabled(name))
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
        self.context_policy(provider, model)
            .input_limit(self.limits.max_input_tokens, window)
    }

    pub fn set_provider(&mut self, name: String, provider: ProviderConfig) {
        self.providers.insert(name, provider);
    }

    pub fn set_max_agents_at_work(&mut self, limit: usize) {
        self.limits.max_agents_at_work = limit;
    }

    pub fn set_max_input_tokens(&mut self, tokens: u64) {
        self.limits.max_input_tokens = tokens;
    }

    pub fn set_max_output_tokens(&mut self, tokens: u64) {
        self.limits.max_output_tokens = tokens;
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
            self.limits.max_input_tokens > 0 && self.limits.max_output_tokens > 0,
            "token limits must be above zero"
        );
        ensure!(
            self.limits.max_agents_at_work > 0,
            "max_agents_at_work must be above zero"
        );
        ensure!(
            self.memory.max_chars > 0,
            "memory.max_chars must be above zero"
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

impl RecordMode {
    pub fn records_reasoning(self) -> bool {
        match self {
            Self::Full => true,
            Self::Answer => false,
        }
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

impl fmt::Debug for ProviderConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderConfig")
            .field("kind", &self.kind)
            .field("base_url", &"[REDACTED]")
            .field("api_key", &self.api_key)
            .finish()
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
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

fn yes() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !value
}

#[cfg(test)]
mod tests {
    #[test]
    fn auto_takes_the_model_window() {
        let auto = ContextPolicy::Auto;
        assert_eq!(auto.input_limit(100_000, Some(256_000)), 256_000);
        assert_eq!(auto.input_limit(100_000, None), 100_000);
        let manual = ContextPolicy::Manual(300_000);
        assert_eq!(manual.input_limit(100_000, Some(256_000)), 256_000);
        assert_eq!(manual.input_limit(100_000, None), 300_000);
    }

    use super::*;

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
        let yaml = config.to_yaml().unwrap();
        assert!(yaml.starts_with(HEADER));
        let again = parse(&yaml).unwrap();
        assert!(!again.tool_enabled("run_command"));
        assert_eq!(again.providers()["main"].api_key.0, "sk-test");
    }
}
