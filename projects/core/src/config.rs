//! The settings in `config.yml`: providers, MCP servers, what threads record and which
//! tools and skills are on.
//! `LiveConfig` keeps the current copy in memory and writes changes back.

use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io::Read,
    path::Path,
};

use anyhow::{Context, Result, anyhow, ensure};
use serde::{Deserialize, Serialize};

use crate::{modes, tools::ToolKind, update::Channel};

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
    /// MCP servers by name, started with the core.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    mcp: BTreeMap<String, McpServerConfig>,
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

/// Reasoning effort, sent as `reasoning_effort`; servers that do not reason ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    High,
}

impl Effort {
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];

    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// `default` stands for none: the provider decides.
    pub fn parse(text: &str) -> Result<Option<Self>> {
        if text == "default" {
            return Ok(None);
        }
        Self::ALL
            .into_iter()
            .find(|effort| effort.name() == text)
            .map(Some)
            .with_context(|| format!("unknown effort {text}; low, medium, high or default"))
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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProviderKind {
    #[default]
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
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
    /// A request larger than this is not sent; the context is compacted well before it.
    /// The model window applies too, when it is smaller.
    #[serde(default = "default_token_limit")]
    pub max_input_tokens: u64,
    /// The model is stopped after writing this many tokens in one call.
    #[serde(default = "default_token_limit")]
    pub max_output_tokens: u64,
    /// Maximum number of subagents running across all threads.
    #[serde(default = "default_max_agents_at_work")]
    pub max_agents_at_work: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_tokens: default_token_limit(),
            max_output_tokens: default_token_limit(),
            max_agents_at_work: default_max_agents_at_work(),
        }
    }
}

fn default_token_limit() -> u64 {
    100_000
}

fn default_max_agents_at_work() -> usize {
    8
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

    pub fn update_channel(&self) -> Channel {
        self.update_channel.unwrap_or_default()
    }

    pub fn set_update_channel(&mut self, channel: Channel) {
        self.update_channel = (channel != Channel::Stable).then_some(channel);
    }

    pub fn effort(&self) -> Option<Effort> {
        self.effort
    }

    pub fn set_effort(&mut self, effort: Option<Effort>) {
        self.effort = effort;
    }

    pub fn set_provider(&mut self, name: String, provider: ProviderConfig) {
        self.providers.insert(name, provider);
    }

    pub fn set_max_agents_at_work(&mut self, limit: usize) {
        self.limits.max_agents_at_work = limit;
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

fn is_false(value: &bool) -> bool {
    !value
}

#[cfg(test)]
mod tests {
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
