//! The settings in `config.yml`: providers, what threads record and which tools and
//! skills are on.
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

/// Written above the settings whenever the core saves the file.
const HEADER: &str = "\
# dusk_code settings. The core rewrites this file when settings change at runtime,
# so comments other than this header are not kept. Keep it private (mode 0600).
";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
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
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: ApiKey,
    /// The server leaves `<think>` and `<tool_call>` tags inside the answer text.
    #[serde(default, skip_serializing_if = "is_false")]
    pub inline_tags: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProviderKind {
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApiKey(pub String);

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
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_tokens: default_token_limit(),
            max_output_tokens: default_token_limit(),
        }
    }
}

fn default_token_limit() -> u64 {
    100_000
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

    pub fn skill_enabled(&self, name: &str) -> bool {
        self.skills.get(name).copied().unwrap_or(true)
    }

    pub fn set_skill_enabled(&mut self, name: &str, enabled: bool) {
        self.skills.insert(name.to_owned(), enabled);
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        // YAML errors can quote input values, including secrets. Drop their source.
        let config: Self =
            serde_yaml::from_slice(bytes).map_err(|_| anyhow!("invalid configuration YAML"))?;
        config.validate()?;
        Ok(config)
    }

    pub fn to_yaml(&self) -> Result<String> {
        let body = serde_yaml::to_string(self).context("cannot encode configuration")?;
        Ok(format!("{HEADER}{body}"))
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            !self.providers.is_empty(),
            "at least one provider is required"
        );
        ensure!(
            self.limits.max_input_tokens > 0 && self.limits.max_output_tokens > 0,
            "token limits must be above zero"
        );
        for (name, provider) in &self.providers {
            ensure!(!name.trim().is_empty(), "provider name must not be empty");
            ensure!(
                !provider.base_url.trim().is_empty(),
                "provider base URL must not be empty"
            );
            ensure!(
                !provider.api_key.0.trim().is_empty()
                    && !provider.api_key.0.chars().any(char::is_control),
                "provider API key is empty or invalid"
            );
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
    fn record_mode_defaults_to_full() {
        assert_eq!(parse(PROVIDERS).unwrap().thread().record, RecordMode::Full);
    }

    #[test]
    fn record_mode_answer_skips_reasoning() {
        let config = parse(&format!("{PROVIDERS}thread:\n  record: answer\n")).unwrap();
        assert_eq!(config.thread().record, RecordMode::Answer);
        assert!(!config.thread().record.records_reasoning());
    }

    #[test]
    fn the_older_session_section_still_reads() {
        let config = parse(&format!("{PROVIDERS}session:\n  record: answer\n")).unwrap();
        assert_eq!(config.thread().record, RecordMode::Answer);
    }

    #[test]
    fn unknown_fields_and_values_are_rejected() {
        assert!(parse(&format!("{PROVIDERS}session:\n  record: everything\n")).is_err());
        assert!(parse(&format!("{PROVIDERS}tool: {{}}\n")).is_err());
        assert!(parse("providers: {}\n").is_err(), "no provider");
    }

    #[test]
    fn tools_are_on_unless_switched_off() {
        let mut config = parse(&format!("{PROVIDERS}tools:\n  run_command: false\n")).unwrap();
        assert!(!config.tool_enabled("run_command"));
        assert!(config.tool_enabled("read_file"));
        config.set_tool_enabled("run_command", true);
        assert!(config.tool_enabled("run_command"));
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
