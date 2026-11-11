//! All the secrets in one keychain item. A keychain asks the user for each item it hands out,
//! and again whenever the program that asks is not the one the item was made for; one item
//! per secret meant a question for every key at every start. The vault is one item holding a
//! small JSON map, read once per run and kept in memory; a change reads it afresh first, so
//! another duskcode process's change is not overwritten.

use std::{collections::BTreeMap, path::PathBuf};

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

/// What the vault read in this run, or why it could not.
enum Cached {
    Loaded(BTreeMap<String, String>),
    /// The keychain was locked or refused; it is not asked again until `refresh`, so one
    /// refusal is one question, not one for each secret.
    Failed(String),
}

pub(super) struct Vault {
    backend: Box<dyn Backend>,
    /// The names old items may have, to be moved into the vault when it is first made.
    legacy_names: Vec<String>,
    cache: Mutex<Option<Cached>>,
    /// A file two duskcode processes lock while one of them changes the vault, so a change
    /// is never made on a map the other has already moved on from.
    lock_path: Option<PathBuf>,
}

/// The vault as the keychain has it, and whether every old item could be read into it.
struct Read {
    secrets: BTreeMap<String, String>,
    complete: bool,
}

impl Vault {
    pub(super) fn new(
        backend: Box<dyn Backend>,
        legacy_names: Vec<String>,
        lock_path: Option<PathBuf>,
    ) -> Self {
        let legacy_names = legacy_names
            .into_iter()
            .filter(|name| check_name(name).is_ok())
            .collect();
        Self {
            backend,
            legacy_names,
            cache: Mutex::new(None),
            lock_path,
        }
    }

    /// Runs `work` with the cross-process lock held. Where no lock can be made, it runs
    /// without: a keychain change is rare and the lock only narrows a window.
    fn locked<T>(&self, work: impl FnOnce() -> Result<T>) -> Result<T> {
        let file = self.lock_path.as_ref().and_then(|path| {
            let mut options = std::fs::OpenOptions::new();
            options.create(true).truncate(false).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(path) {
                Ok(file) => file.lock().ok().map(|()| file),
                Err(error) => {
                    warn!("no lock for the keychain vault: {error}");
                    None
                }
            }
        });
        let result = work();
        drop(file);
        result
    }

    /// The map as the keychain has it now. With no vault yet, the old items are moved in.
    /// Called with the lock held.
    fn fresh(&self) -> Result<Read> {
        if let Some(text) = self.backend.read()? {
            // The parser's own message may quote what it could not read, a key among it.
            let contents: Contents = serde_json::from_str(&text)
                .map_err(|_| anyhow!("the keychain item of duskcode is damaged"))?;
            return Ok(Read {
                secrets: contents.secrets,
                complete: true,
            });
        }
        let mut secrets = BTreeMap::new();
        let mut moved = Vec::new();
        let mut complete = true;
        for name in &self.legacy_names {
            match self.backend.legacy(name) {
                Ok(Some(value)) => {
                    secrets.insert(name.clone(), value);
                    moved.push(name.clone());
                }
                Ok(None) => {}
                Err(error) => {
                    warn!("{name} was not read for the vault: {error:#}");
                    complete = false;
                }
            }
        }
        // With an old item unread, the vault is not made: it would stand for the whole and
        // the unread secret would never be looked for again. The next run tries again.
        if secrets.is_empty() || !complete {
            return Ok(Read { secrets, complete });
        }
        // The old items go only once the vault holds what they held.
        self.write(&secrets)?;
        for name in moved {
            if let Err(error) = self.backend.forget_legacy(&name) {
                warn!("the old keychain item {name} stays: {error:#}");
            }
        }
        Ok(Read {
            secrets,
            complete: true,
        })
    }

    /// Writes the map and reads it back from the keychain, so a keychain that says yes and
    /// keeps nothing is found out before the keys leave the config file.
    fn write(&self, secrets: &BTreeMap<String, String>) -> Result<()> {
        let text = serde_json::to_string(&Contents {
            secrets: secrets.clone(),
        })?;
        self.backend.write(&text)?;
        let stored = self.backend.read()?;
        anyhow::ensure!(
            stored.as_deref() == Some(text.as_str()),
            "the keychain did not keep what was written"
        );
        Ok(())
    }

    /// The map for a change: the keychain's own, and an error while an old item is unread.
    fn for_change(&self) -> Result<BTreeMap<String, String>> {
        let read = self.fresh()?;
        anyhow::ensure!(
            read.complete,
            "some old keychain items could not be read; allow duskcode access to them and try again"
        );
        Ok(read.secrets)
    }
}

impl SecretStore for Vault {
    fn get(&self, name: &str) -> Result<Option<String>> {
        check_name(name)?;
        let mut cache = self.cache.lock();
        if cache.is_none() {
            *cache = Some(match self.locked(|| self.fresh()) {
                Ok(read) => Cached::Loaded(read.secrets),
                Err(error) => Cached::Failed(format!("{error:#}")),
            });
        }
        match cache.as_ref() {
            Some(Cached::Loaded(secrets)) => Ok(secrets.get(name).cloned()),
            Some(Cached::Failed(why)) => Err(anyhow!("{why}")),
            None => Ok(None),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        check_name(name)?;
        let mut cache = self.cache.lock();
        let secrets = self.locked(|| {
            let mut secrets = self.for_change()?;
            secrets.insert(name.to_owned(), value.to_owned());
            self.write(&secrets)
                .map_err(|error| anyhow!("cannot save {name} to the keychain: {error:#}"))?;
            Ok(secrets)
        })?;
        *cache = Some(Cached::Loaded(secrets));
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<()> {
        check_name(name)?;
        let mut cache = self.cache.lock();
        let secrets = self.locked(|| {
            let mut secrets = self.for_change()?;
            if secrets.remove(name).is_some() {
                self.write(&secrets).map_err(|error| {
                    anyhow!("cannot delete {name} from the keychain: {error:#}")
                })?;
            }
            Ok(secrets)
        })?;
        *cache = Some(Cached::Loaded(secrets));
        Ok(())
    }

    fn refresh(&self) {
        *self.cache.lock() = None;
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
            None,
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

    #[test]
    fn a_refresh_lets_a_change_of_another_process_through() {
        let fake = Arc::new(Fake::default());
        let mine = vault(&fake, &[]);
        mine.set("provider/a", "old").unwrap();
        vault(&fake, &[]).set("provider/a", "rotated").unwrap();
        assert_eq!(mine.get("provider/a").unwrap().as_deref(), Some("old"));

        mine.refresh();

        assert_eq!(mine.get("provider/a").unwrap().as_deref(), Some("rotated"));
    }

    /// An old item the user refuses to hand over.
    struct Refusing(Arc<Fake>);

    impl Backend for Refusing {
        fn read(&self) -> Result<Option<String>> {
            self.0.read()
        }
        fn write(&self, text: &str) -> Result<()> {
            self.0.write(text)
        }
        fn legacy(&self, name: &str) -> Result<Option<String>> {
            if name == "provider/b" {
                anyhow::bail!("access denied");
            }
            self.0.legacy(name)
        }
        fn forget_legacy(&self, name: &str) -> Result<()> {
            self.0.forget_legacy(name)
        }
    }

    #[test]
    fn an_old_item_that_cannot_be_read_keeps_the_vault_from_being_made() {
        let fake = Arc::new(Fake::default());
        fake.old.lock().insert("provider/a".into(), "key-a".into());
        fake.old.lock().insert("provider/b".into(), "key-b".into());
        let store = Vault::new(
            Box::new(Refusing(Arc::clone(&fake))),
            vec!["provider/a".into(), "provider/b".into()],
            None,
        );

        // What could be read is served, but nothing is written and nothing is deleted.
        assert_eq!(store.get("provider/a").unwrap().as_deref(), Some("key-a"));
        assert!(fake.vault.lock().is_none());
        assert_eq!(fake.old.lock().len(), 2);
        // A change is refused until the refused item can be read, so it is not left behind.
        assert!(store.set("provider/c", "3").is_err());
        assert!(fake.vault.lock().is_none());
    }

    /// A keychain that says yes to a write and keeps nothing.
    struct Forgetful(Arc<Fake>);

    impl Backend for Forgetful {
        fn read(&self) -> Result<Option<String>> {
            self.0.read()
        }
        fn write(&self, _: &str) -> Result<()> {
            Ok(())
        }
        fn legacy(&self, name: &str) -> Result<Option<String>> {
            self.0.legacy(name)
        }
        fn forget_legacy(&self, name: &str) -> Result<()> {
            self.0.forget_legacy(name)
        }
    }

    #[test]
    fn a_keychain_that_keeps_nothing_is_found_out() {
        let fake = Arc::new(Fake::default());
        let store = Vault::new(Box::new(Forgetful(fake)), Vec::new(), None);
        assert!(store.set("provider/a", "1").is_err());
    }

    #[test]
    fn a_refusal_is_one_question_not_one_for_each_secret() {
        let fake = Arc::new(Fake::default());
        *fake.vault.lock() = Some("not json".into());
        let store = vault(&fake, &[]);
        for _ in 0..4 {
            assert!(store.get("provider/a").is_err());
        }
        assert_eq!(fake.reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_error_of_a_damaged_vault_does_not_quote_it() {
        let fake = Arc::new(Fake::default());
        *fake.vault.lock() = Some(r#"{"secrets":"sk-live-secret"}"#.into());
        let error = vault(&fake, &[]).set("provider/a", "1").unwrap_err();
        assert!(!format!("{error:#}").contains("sk-live-secret"));
    }
}
