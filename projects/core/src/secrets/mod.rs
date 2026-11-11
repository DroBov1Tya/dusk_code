//! Secrets in the system keychain: macOS Keychain, the Secret Service on Linux, the
//! Credential Manager on Windows. A secret has a name and a text value; the names are
//! `provider/<name>` for a provider's API key and `huggingface` for the Hugging Face
//! token. All of them live in one keychain item, the vault, so the user is asked once. Without a keychain nothing is stored here, and the provider keys stay in the
//! private config file as they were.

use anyhow::{Result, ensure};

pub use dusk_protocol::config::HUGGINGFACE;

mod forget;
mod keychain;
#[cfg(test)]
mod memory;
mod vault;

pub use forget::forget_all;
pub use keychain::Keychain;
#[cfg(test)]
pub use memory::MemoryStore;

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
        let store =
            Keychain::open(std::path::Path::new("/nonexistent/duskcode-test")).expect("a keychain");
        let name = "test/round-trip";
        store.set(name, "value").unwrap();
        assert_eq!(store.get(name).unwrap().as_deref(), Some("value"));
        store.delete(name).unwrap();
        assert_eq!(store.get(name).unwrap(), None);
    }
}
