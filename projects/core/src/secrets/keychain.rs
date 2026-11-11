//! The machine's keychain, through the keyring crate: one item, the vault.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};
use tracing::warn;

use super::{
    HUGGINGFACE, SecretStore, provider_key,
    vault::{Backend, Vault},
};

const SERVICE: &str = "duskcode";
/// The name of the one item that holds every secret.
const VAULT_ITEM: &str = "vault";

pub struct Keychain;

/// The keychain service of a config folder: the standard one for `~/.config/duskcode`, a
/// service of its own for any other folder, so that a second install, a test run or a
/// throwaway HOME never shares or deletes the keys of the first.
fn service_for(root: &Path) -> String {
    // A link or a relative path to the same folder is the same install.
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let standard = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".config/duskcode"))
        .map(|path| std::fs::canonicalize(&path).unwrap_or(path));
    if standard.as_deref() == Some(root.as_path()) {
        return SERVICE.to_owned();
    }
    let digest = Sha256::digest(root.as_os_str().as_encoded_bytes());
    let short: String = digest
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{SERVICE}-{short}")
}

/// The names items of older versions may have: the fixed ones and one for each provider of the
/// config file, which names them even though it no longer holds their keys.
fn legacy_names(root: &Path) -> Vec<String> {
    use dusk_protocol::config::{STT_KEY, TTS_KEY};
    let mut names = vec![
        HUGGINGFACE.to_owned(),
        STT_KEY.to_owned(),
        TTS_KEY.to_owned(),
    ];
    if let Ok(config) = crate::config::Config::load_from(root.join("config.yml")) {
        names.extend(config.providers().keys().map(|name| provider_key(name)));
    }
    names
}

struct KeyringBackend {
    service: String,
}

impl KeyringBackend {
    fn entry(&self, item: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, item).map_err(|error| anyhow!("keychain: {error}"))
    }

    fn read_item(&self, item: &str) -> Result<Option<String>> {
        match self.entry(item)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(anyhow!("cannot read {item} from the keychain: {error}")),
        }
    }

    fn delete_item(&self, item: &str) -> Result<()> {
        match self.entry(item)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(anyhow!("cannot delete {item} from the keychain: {error}")),
        }
    }
}

impl Backend for KeyringBackend {
    fn read(&self) -> Result<Option<String>> {
        self.read_item(VAULT_ITEM)
    }

    fn write(&self, text: &str) -> Result<()> {
        self.entry(VAULT_ITEM)?
            .set_password(text)
            .map_err(|error| anyhow!("cannot write the vault to the keychain: {error}"))
    }

    fn legacy(&self, name: &str) -> Result<Option<String>> {
        self.read_item(name)
    }

    fn forget_legacy(&self, name: &str) -> Result<()> {
        self.delete_item(name)
    }
}

impl Keychain {
    /// The machine's keychain for the config folder `root`, or `None` when it has none or
    /// cannot open it. Nothing is read yet: the keychain is asked on the first secret.
    pub fn open(root: &Path) -> Option<Arc<dyn SecretStore>> {
        match keyring::Entry::store_status() {
            Ok(()) => {
                let backend = KeyringBackend {
                    service: service_for(root),
                };
                Some(Arc::new(Vault::new(
                    Box::new(backend),
                    legacy_names(root),
                    Some(root.join("secrets.lock")),
                )))
            }
            Err(error) => {
                warn!("no system keychain; keys cannot be moved out of the config file: {error}");
                None
            }
        }
    }

    /// Deletes the vault and every old item of the config folder `root`; reports what stays.
    pub fn erase(root: &Path) -> Result<()> {
        if keyring::Entry::store_status().is_err() {
            return Ok(());
        }
        let backend = KeyringBackend {
            service: service_for(root),
        };
        let mut left: Vec<String> = Vec::new();
        for item in std::iter::once(VAULT_ITEM.to_owned()).chain(legacy_names(root)) {
            if backend.delete_item(&item).is_err() {
                left.push(item);
            }
        }
        anyhow::ensure!(
            left.is_empty(),
            "these stay in the keychain: {}",
            left.join(", ")
        );
        Ok(())
    }
}
