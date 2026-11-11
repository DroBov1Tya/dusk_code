//! All the secrets in one keychain item. A keychain asks the user for each item it hands out,
//! and again whenever the program that asks is not the one the item was made for; one item
//! per secret meant a question for every key at every start. The vault is one item holding a
//! small JSON map, read once per run and kept in memory; a change reads it afresh first, so
//! another duskcode process's change is not overwritten.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::warn;

use super::{SecretStore, check_name};

/// Where the vault and the items older versions made are kept.
pub(super) trait Backend: Send + Sync {
    /// The vault's text, or `None` when there is none yet.
    fn read(&self) -> Result<Option<String>>;
    fn write(&self, text: &str) -> Result<()>;
    /// An item of the old kind: one secret by itself.
    fn legacy(&self, name: &str) -> Result<Option<String>>;
    fn forget_legacy(&self, name: &str) -> Result<()>;
}

#[derive(Default, Serialize, Deserialize)]
struct Contents {
    #[serde(default)]
    secrets: BTreeMap<String, String>,
}

pub(super) struct Vault {
    backend: Box<dyn Backend>,
    /// The names old items may have, to be moved into the vault when it is first made.
    legacy_names: Vec<String>,
    cache: Mutex<Option<BTreeMap<String, String>>>,
}

impl Vault {
    pub(super) fn new(backend: Box<dyn Backend>, legacy_names: Vec<String>) -> Self {
        Self {
            backend,
            legacy_names,
            cache: Mutex::new(None),
        }
    }

    /// The map as the keychain has it now. With no vault yet, the old items are moved in.
    fn fresh(&self) -> Result<BTreeMap<String, String>> {
        if let Some(text) = self.backend.read()? {
            let contents: Contents =
                serde_json::from_str(&text).context("the keychain item of duskcode is damaged")?;
            return Ok(contents.secrets);
        }
        let mut secrets = BTreeMap::new();
        let mut moved = Vec::new();
        for name in &self.legacy_names {
            match self.backend.legacy(name) {
                Ok(Some(value)) => {
                    secrets.insert(name.clone(), value);
                    moved.push(name.clone());
                }
                Ok(None) => {}
                Err(error) => warn!("{name} was not moved into the vault: {error:#}"),
            }
        }
        if secrets.is_empty() {
            return Ok(secrets);
        }
        // The old items go only once the vault holds what they held.
        self.write(&secrets)?;
        for name in moved {
            if let Err(error) = self.backend.forget_legacy(&name) {
                warn!("the old keychain item {name} stays: {error:#}");
            }
        }
        Ok(secrets)
    }

    fn write(&self, secrets: &BTreeMap<String, String>) -> Result<()> {
        let text = serde_json::to_string(&Contents {
            secrets: secrets.clone(),
        })?;
        self.backend.write(&text)
    }
}

impl SecretStore for Vault {
    fn get(&self, name: &str) -> Result<Option<String>> {
        check_name(name)?;
        let mut cache = self.cache.lock();
        if cache.is_none() {
            *cache = Some(self.fresh()?);
        }
        Ok(cache
            .as_ref()
            .and_then(|secrets| secrets.get(name).cloned()))
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        check_name(name)?;
        let mut cache = self.cache.lock();
        let mut secrets = self.fresh()?;
        secrets.insert(name.to_owned(), value.to_owned());
        self.write(&secrets)
            .map_err(|error| anyhow!("cannot save {name} to the keychain: {error:#}"))?;
        *cache = Some(secrets);
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<()> {
        check_name(name)?;
        let mut cache = self.cache.lock();
        let mut secrets = self.fresh()?;
        if secrets.remove(name).is_some() {
            self.write(&secrets)
                .map_err(|error| anyhow!("cannot delete {name} from the keychain: {error:#}"))?;
        }
        *cache = Some(secrets);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use super::*;

    /// A keychain in memory that counts how often the vault is read.
    #[derive(Default)]
    struct Fake {
        vault: Mutex<Option<String>>,
        old: Mutex<HashMap<String, String>>,
        reads: AtomicUsize,
    }

    impl Backend for Arc<Fake> {
        fn read(&self) -> Result<Option<String>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self.vault.lock().clone())
        }
        fn write(&self, text: &str) -> Result<()> {
            *self.vault.lock() = Some(text.to_owned());
            Ok(())
        }
        fn legacy(&self, name: &str) -> Result<Option<String>> {
            Ok(self.old.lock().get(name).cloned())
        }
        fn forget_legacy(&self, name: &str) -> Result<()> {
            self.old.lock().remove(name);
            Ok(())
        }
    }

    fn vault(fake: &Arc<Fake>, names: &[&str]) -> Vault {
        Vault::new(
            Box::new(Arc::clone(fake)),
            names.iter().map(|name| (*name).to_owned()).collect(),
        )
    }

    #[test]
    fn many_secrets_cost_one_read_of_the_keychain() {
        let fake = Arc::new(Fake::default());
        let store = vault(&fake, &[]);
        store.set("provider/a", "1").unwrap();
        store.set("provider/b", "2").unwrap();
        let reads = fake.reads.load(Ordering::SeqCst);

        for _ in 0..5 {
            assert_eq!(store.get("provider/a").unwrap().as_deref(), Some("1"));
            assert_eq!(store.get("provider/b").unwrap().as_deref(), Some("2"));
            assert_eq!(store.get("huggingface").unwrap(), None);
        }
        // The first get is served from what the last set left in memory.
        assert_eq!(fake.reads.load(Ordering::SeqCst), reads);
    }

    #[test]
    fn a_fresh_run_reads_the_keychain_once_for_all_its_gets() {
        let fake = Arc::new(Fake::default());
        vault(&fake, &[]).set("provider/a", "1").unwrap();
        fake.reads.store(0, Ordering::SeqCst);

        let store = vault(&fake, &[]);
        for _ in 0..4 {
            store.get("provider/a").unwrap();
            store.get("huggingface").unwrap();
        }
        assert_eq!(fake.reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn old_items_move_into_the_vault_and_are_forgotten() {
        let fake = Arc::new(Fake::default());
        fake.old.lock().insert("provider/a".into(), "key-a".into());
        fake.old
            .lock()
            .insert("huggingface".into(), "hf_token".into());

        let store = vault(&fake, &["provider/a", "provider/gone", "huggingface"]);
        assert_eq!(store.get("provider/a").unwrap().as_deref(), Some("key-a"));
        assert_eq!(
            store.get("huggingface").unwrap().as_deref(),
            Some("hf_token")
        );
        assert!(fake.old.lock().is_empty(), "the old items were deleted");
        // A later run finds them in the vault, not in old items.
        assert_eq!(
            vault(&fake, &[]).get("provider/a").unwrap().as_deref(),
            Some("key-a")
        );
    }

    #[test]
    fn a_change_does_not_overwrite_what_another_process_wrote() {
        let fake = Arc::new(Fake::default());
        let mine = vault(&fake, &[]);
        mine.set("provider/a", "1").unwrap();
        // Another duskcode process adds a secret behind this one's back.
        vault(&fake, &[]).set("provider/b", "2").unwrap();

        mine.set("provider/c", "3").unwrap();

        let all = vault(&fake, &[]);
        assert_eq!(all.get("provider/a").unwrap().as_deref(), Some("1"));
        assert_eq!(all.get("provider/b").unwrap().as_deref(), Some("2"));
        assert_eq!(all.get("provider/c").unwrap().as_deref(), Some("3"));
    }

    #[test]
    fn a_deleted_secret_is_gone_and_a_damaged_vault_is_not_replaced_silently() {
        let fake = Arc::new(Fake::default());
        let store = vault(&fake, &[]);
        store.set("provider/a", "1").unwrap();
        store.delete("provider/a").unwrap();
        assert_eq!(store.get("provider/a").unwrap(), None);

        *fake.vault.lock() = Some("not json".into());
        assert!(vault(&fake, &[]).get("provider/a").is_err());
        assert!(vault(&fake, &[]).set("provider/a", "1").is_err());
    }
}
