//! User-local provider settings, stored at `Paths::config_file`. Model selection belongs
//! to runtime callers.

use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io::Read,
    path::Path,
};

use anyhow::{Result, anyhow, ensure};
use serde::Deserialize;

use crate::paths::Paths;

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    session: SessionConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionConfig {
    #[serde(default)]
    pub record: RecordMode,
}

/// What a session stores from the model output. User messages are always stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordMode {
    /// Everything the model produces: reasoning, tool calls and the answer.
    #[default]
    Full,
    /// Only the answer.
    Answer,
}

impl RecordMode {
    pub fn records_reasoning(self) -> bool {
        match self {
            Self::Full => true,
            Self::Answer => false,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: String,
    pub(crate) api_key: ApiKey,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum ProviderKind {
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
}

#[derive(Clone, Deserialize)]
#[serde(transparent)]
pub(crate) struct ApiKey(pub(crate) String);

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("providers", &"[REDACTED]")
            .field("provider_count", &self.providers.len())
            .field("session", &self.session)
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

impl Config {
    pub fn load(paths: &Paths) -> Result<Self> {
        Self::load_from(paths.config_file())
    }

    /// Load a private local file; this method never writes config or model IDs.
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
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

        let metadata =
            fs::symlink_metadata(path).map_err(|_| anyhow!("cannot inspect config file"))?;
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
        file.take(MAX_CONFIG_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| anyhow!("cannot read config file"))?;
        ensure!(
            bytes.len() as u64 <= MAX_CONFIG_BYTES,
            "config file is too large"
        );

        // YAML errors can quote input values, including secrets. Drop their source.
        let config: Self = serde_yaml::from_slice(&bytes)
            .map_err(|_| anyhow!("invalid provider configuration YAML"))?;
        ensure!(
            !config.providers.is_empty(),
            "at least one provider is required"
        );
        for (name, provider) in &config.providers {
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
        Ok(config)
    }

    pub fn providers(&self) -> &BTreeMap<String, ProviderConfig> {
        &self.providers
    }

    pub fn session(&self) -> &SessionConfig {
        &self.session
    }
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

    const PROVIDERS: &str = "providers:\n  main:\n    kind: openai_compatible\n    base_url: 'https://host/v1'\n    api_key: 'sk-test'\n";

    fn parse(yaml: &str) -> Result<Config, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }

    #[test]
    fn record_mode_defaults_to_full() {
        assert_eq!(parse(PROVIDERS).unwrap().session().record, RecordMode::Full);
    }

    #[test]
    fn record_mode_answer_skips_reasoning() {
        let config = parse(&format!("{PROVIDERS}session:\n  record: answer\n")).unwrap();
        assert_eq!(config.session().record, RecordMode::Answer);
        assert!(!config.session().record.records_reasoning());
    }

    #[test]
    fn unknown_record_mode_is_rejected() {
        assert!(parse(&format!("{PROVIDERS}session:\n  record: everything\n")).is_err());
    }
}
