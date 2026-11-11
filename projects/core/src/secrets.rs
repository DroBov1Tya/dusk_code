//! Secrets in the system keychain: macOS Keychain, the Secret Service on Linux, the
//! Credential Manager on Windows. A secret has a name and a text value; the names are
//! `provider/<name>` for a provider's API key and `huggingface` for the Hugging Face
//! token. Without a keychain nothing is stored here, and the provider keys stay in the
//! private config file as they were.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Result, anyhow, ensure};
use sha2::{Digest, Sha256};
use tracing::warn;

pub use dusk_protocol::config::HUGGINGFACE;

const SERVICE: &str = "duskcode";
const MAX_NAME: usize = 200;

pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>>;
    fn set(&self, name: &str, value: &str) -> Result<()>;
    /// A secret that is not there is already deleted.
    fn delete(&self, name: &str) -> Result<()>;
}

/// The name a provider's API key is kept under.
pub fn provider_key(provider: &str) -> String {
    format!("provider/{provider}")
}

fn check_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name.len() <= MAX_NAME && !name.chars().any(char::is_control),
        "a secret's name is up to {MAX_NAME} characters, without control characters"
    );
    Ok(())
}

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

/// Deletes every secret duskcode keeps for the config file: the Hugging Face token and the
/// key of each provider the file names. It goes on past a failure and reports the names
/// left, so a removal is never stopped half way by a locked keychain.
pub fn forget_all(config_file: &Path) -> Result<()> {
    let root = config_file.parent().unwrap_or(config_file);
    let Some(store) = Keychain::open(root) else {
        return Ok(());
    };
    let mut names = vec![HUGGINGFACE.to_owned()];
    // A config that cannot be read names no providers; their keys are then left behind.
    if let Ok(config) = crate::config::Config::load_from(config_file) {
        names.extend(config.providers().keys().map(|name| provider_key(name)));
    }
    let left: Vec<_> = names
        .into_iter()
        .filter(|name| store.delete(name).is_err())
        .collect();
    ensure!(
        left.is_empty(),
        "these stay in the keychain: {}",
        left.join(", ")
    );
    Ok(())
}

/// A store in memory, for tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(parking_lot::Mutex<std::collections::HashMap<String, String>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        check_name(name)?;
        Ok(self.0.lock().get(name).cloned())
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        check_name(name)?;
        self.0.lock().insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<()> {
        self.0.lock().remove(name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_without_text_or_with_control_characters_is_refused() {
        assert!(check_name("").is_err());
        assert!(check_name("a\nb").is_err());
        assert!(check_name(&"x".repeat(MAX_NAME + 1)).is_err());
        assert!(check_name("provider/main").is_ok());
    }

    /// Needs a keychain and may ask the user to allow it, so it runs on request:
    /// `cargo test -p dusk_core keychain_round_trip -- --ignored`.
    #[test]
    #[ignore]
    fn keychain_round_trip() {
        let store = Keychain::open(Path::new("/nonexistent/duskcode-test")).expect("a keychain");
        let name = "test/round-trip";
        store.set(name, "value").unwrap();
        assert_eq!(store.get(name).unwrap().as_deref(), Some("value"));
        store.delete(name).unwrap();
        assert_eq!(store.get(name).unwrap(), None);
    }
}
