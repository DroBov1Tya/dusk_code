//! The machine's keychain, through the keyring crate.

use super::{SecretStore, check_name};
use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::warn;

const SERVICE: &str = "duskcode";

pub struct Keychain {
    service: String,
}

/// The keychain service of a config folder: the standard one for `~/.config/duskcode`, a
/// service of its own for any other folder, so that a second install, a test run or a
/// throwaway HOME never shares or deletes the keys of the first.
fn service_for(root: &Path) -> String {
    let standard =
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/duskcode"));
    if standard.as_deref() == Some(root) {
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

impl Keychain {
    /// The machine's keychain for the config folder `root`, or `None` when it has none or
    /// cannot open it.
    pub fn open(root: &Path) -> Option<Arc<dyn SecretStore>> {
        match keyring::Entry::store_status() {
            Ok(()) => Some(Arc::new(Self {
                service: service_for(root),
            })),
            Err(error) => {
                warn!("no system keychain; keys cannot be moved out of the config file: {error}");
                None
            }
        }
    }

    fn entry(&self, name: &str) -> Result<keyring::Entry> {
        check_name(name)?;
        keyring::Entry::new(&self.service, name).map_err(|error| anyhow!("keychain: {error}"))
    }
}

impl SecretStore for Keychain {
    fn get(&self, name: &str) -> Result<Option<String>> {
        match self.entry(name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(anyhow!("cannot read {name} from the keychain: {error}")),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        self.entry(name)?
            .set_password(value)
            .map_err(|error| anyhow!("cannot save {name} to the keychain: {error}"))
    }

    fn delete(&self, name: &str) -> Result<()> {
        match self.entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(anyhow!("cannot delete {name} from the keychain: {error}")),
        }
    }
}
