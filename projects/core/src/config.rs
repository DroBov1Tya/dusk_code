//! User-local provider settings. Model selection belongs to runtime callers.

use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow, ensure};
use serde::Deserialize;

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    providers: BTreeMap<String, ProviderConfig>,
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
    /// Use the requested `~/.config/duskcode/config.yml` path on every platform.
    /// `XDG_CONFIG_HOME` is deliberately ignored to keep this location stable.
    pub fn path() -> Result<PathBuf> {
        let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
        let home = PathBuf::from(home);
        ensure!(home.is_absolute(), "HOME must be an absolute path");
        Ok(home.join(".config/duskcode/config.yml"))
    }

    pub fn load() -> Result<Self> {
        Self::load_from(Self::path()?)
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
