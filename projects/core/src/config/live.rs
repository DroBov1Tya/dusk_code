//! The config as the running core sees it. The current copy lives in memory behind a
//! lock; readers take a snapshot and never hold the lock while they work. Changes made
//! through `update` are written to disk first, and edits made to the file by hand are
//! picked up by `reload` or by `watch` in the background.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use parking_lot::{Mutex, MutexGuard, RwLock};

use anyhow::{Context, Result};
use tracing::{info, warn};

use crate::{
    config::{Config, Plugged},
    secrets::{SecretStore, provider_key},
};

pub struct LiveConfig {
    path: PathBuf,
    current: RwLock<Arc<Config>>,
    /// What the plugins add; laid over every version of the file.
    plugged: RwLock<Plugged>,
    /// Serializes reloads and writes, and remembers which file version is loaded.
    loaded: Mutex<FileStamp>,
    /// Where the providers' API keys are kept, when the machine has a keychain. The file
    /// then holds none; the copy in memory does.
    secrets: Option<Arc<dyn SecretStore>>,
}

/// Enough to notice that the file changed without reading it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: SystemTime,
    len: u64,
}

impl LiveConfig {
    pub fn load(path: impl Into<PathBuf>) -> Result<Self> {
        Self::load_with(path, None)
    }

    /// Loads the config with `secrets` keeping the API keys. Keys still in the file move
    /// to the store at once.
    pub fn load_with(
        path: impl Into<PathBuf>,
        secrets: Option<Arc<dyn SecretStore>>,
    ) -> Result<Self> {
        let path = path.into();
        let stamp = FileStamp::of(&path)?;
        let config = Config::load_from(&path)?;
        let live = Self {
            path,
            current: RwLock::new(Arc::new(config)),
            plugged: RwLock::default(),
            loaded: Mutex::new(stamp),
            secrets,
        };
        live.reload()?;
        Ok(live)
    }

    pub fn secrets(&self) -> Option<&Arc<dyn SecretStore>> {
        self.secrets.as_ref()
    }

    /// A snapshot: later changes do not affect it.
    pub fn current(&self) -> Arc<Config> {
        Arc::clone(&self.current.read())
    }

    /// Reads the file now, for example after something was added through a client.
    pub fn reload(&self) -> Result<()> {
        let mut loaded = self.lock_loaded();
        // Another process may have changed a key since: ask the keychain again.
        if let Some(store) = &self.secrets {
            store.refresh();
        }
        self.read_from_disk(&mut loaded)
    }

    /// Changes the config on disk and in memory. The file is read first, so a hand edit
    /// made since the last reload is kept rather than overwritten.
    pub fn update(&self, change: impl FnOnce(&mut Config)) -> Result<Arc<Config>> {
        let mut loaded = self.lock_loaded();
        self.read_from_disk(&mut loaded)?;
        let before = self.current();
        let mut config = Config::clone(&before);
        change(&mut config);
        // New keys are saved first, so a failure here changes nothing; the keys of what is
        // gone are deleted only once the file is written.
        let doomed = self.store_keys(&before, &config)?;
        // With a keychain the file carries no key.
        let yaml = match self.secrets {
            Some(_) => config.without_keys().to_yaml()?,
            None => config.to_yaml()?,
        };
        // Parsing what will be written proves the file will load again.
        Config::parse(yaml.as_bytes())?;
        crate::util::atomic::write_private(&self.path, yaml.as_bytes())?;
        *loaded = FileStamp::of(&self.path)?;
        self.delete_keys(&doomed);
        config.set_plugged(self.plugged.read().clone());
        let config = Arc::new(config);
        *self.current.write() = Arc::clone(&config);
        Ok(config)
    }

    /// Replaces what the plugins add, in memory only.
    pub fn set_plugged(&self, plugged: Plugged) {
        let _loaded = self.lock_loaded();
        let mut config = Config::clone(&self.current());
        config.set_plugged(plugged.clone());
        *self.plugged.write() = plugged;
        *self.current.write() = Arc::new(config);
    }

    /// Re-reads the file whenever it changes. A broken file is reported and the last
    /// good config stays in use.
    pub async fn watch(self: Arc<Self>, period: Duration) {
        let mut ticks = tokio::time::interval(period);
        loop {
            ticks.tick().await;
            if let Err(error) = self.reload_if_changed() {
                warn!("config not reloaded: {error:#}");
            }
        }
    }

    fn reload_if_changed(&self) -> Result<()> {
        let mut loaded = self.lock_loaded();
        let stamp = FileStamp::of(&self.path)?;
        if stamp == *loaded {
            return Ok(());
        }
        // Remembered before parsing, so a broken version is reported once, not every tick.
        *loaded = stamp;
        // A change of the file may come with a change of a key made by another process.
        if let Some(store) = &self.secrets {
            store.refresh();
        }
        self.read_from_disk(&mut loaded)
    }

    fn read_from_disk(&self, loaded: &mut FileStamp) -> Result<()> {
        let stamp = FileStamp::of(&self.path)?;
        let mut config = Config::load_from(&self.path)?;
        let mut stamp = stamp;
        if let Some(store) = &self.secrets {
            if self.move_keys_out(store.as_ref(), &config) {
                // The file was rewritten without the keys.
                stamp = FileStamp::of(&self.path)?;
            }
            // A provider removed by hand takes its key along, so a new one of the same name
            // never inherits it.
            let gone: Vec<String> = self
                .current()
                .providers()
                .keys()
                .filter(|name| !config.providers().contains_key(*name))
                .map(|name| provider_key(name))
                .collect();
            self.delete_keys(&gone);
            fill_keys(&mut config, store.as_ref());
        }
        config.set_plugged(self.plugged.read().clone());
        *self.current.write() = Arc::new(config);
        *loaded = stamp;
        Ok(())
    }

    /// Moves the keys found in the file to the store, and rewrites the file without them.
    /// The file is rewritten only after every key is read back from the store, so a key is
    /// never lost. True when the file was rewritten.
    fn move_keys_out(&self, store: &dyn SecretStore, config: &Config) -> bool {
        let found: Vec<_> = config
            .providers()
            .iter()
            .filter(|(_, provider)| !provider.api_key.0.is_empty())
            .collect();
        if found.is_empty() {
            return false;
        }
        let stored = found.iter().try_for_each(|(name, provider)| {
            let secret = provider_key(name);
            store.set(&secret, &provider.api_key.0)?;
            anyhow::ensure!(
                store.get(&secret)?.as_deref() == Some(provider.api_key.0.as_str()),
                "{secret} did not read back from the keychain"
            );
            anyhow::Ok(())
        });
        let written = stored.and_then(|()| {
            let yaml = config.without_keys().to_yaml()?;
            crate::util::atomic::write_private(&self.path, yaml.as_bytes())
        });
        match written {
            Ok(()) => {
                info!("moved {} provider keys to the system keychain", found.len());
                true
            }
            Err(error) => {
                warn!("provider keys stay in the config file: {error:#}");
                false
            }
        }
    }

    /// Saves the keys a change adds or alters, and returns the secrets it leaves without
    /// an owner: those of providers that are gone and of keys that were cleared. Only keys
    /// that differ are touched, so a locked keychain does not block unrelated changes.
    fn store_keys(&self, before: &Config, after: &Config) -> Result<Vec<String>> {
        let Some(store) = &self.secrets else {
            return Ok(Vec::new());
        };
        let mut doomed = Vec::new();
        for (name, provider) in after.providers() {
            let key = &provider.api_key.0;
            let old = before
                .providers()
                .get(name)
                .map(|old| old.api_key.0.as_str());
            if old == Some(key.as_str()) {
                continue;
            }
            if key.is_empty() {
                doomed.push(provider_key(name));
            } else {
                store.set(&provider_key(name), key)?;
            }
        }
        for name in before.providers().keys() {
            if !after.providers().contains_key(name) {
                doomed.push(provider_key(name));
            }
        }
        Ok(doomed)
    }

    fn delete_keys(&self, names: &[String]) {
        let Some(store) = &self.secrets else { return };
        for name in names {
            if let Err(error) = store.delete(name) {
                warn!("{name} stays in the keychain: {error:#}");
            }
        }
    }

    fn lock_loaded(&self) -> MutexGuard<'_, FileStamp> {
        // The guarded value is a plain stamp, still valid after a panic elsewhere.
        self.loaded.lock()
    }
}

/// Gives each provider without a key the one the store keeps for it.
fn fill_keys(config: &mut Config, store: &dyn SecretStore) {
    for (name, key) in config.provider_keys_mut() {
        if !key.0.is_empty() {
            continue;
        }
        match store.get(&provider_key(name)) {
            Ok(Some(value)) => key.0 = value,
            Ok(None) => {}
            Err(error) => warn!("no key for provider {name}: {error:#}"),
        }
    }
}

impl FileStamp {
    fn of(path: &Path) -> Result<Self> {
        let metadata = fs::metadata(path).context("cannot inspect config file")?;
        Ok(Self {
            modified: metadata
                .modified()
                .context("cannot read config file time")?,
            len: metadata.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    const CONFIG: &str = "providers:\n  main:\n    kind: openai_compatible\n    base_url: 'https://host/v1'\n    api_key: 'sk-test'\n";

    fn config_file(name: &str, yaml: &str) -> PathBuf {
        let directory = temp_dir(name);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.yml");
        crate::util::atomic::write_private(&path, yaml.as_bytes()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        }
        path
    }

    #[test]
    fn update_keeps_a_hand_edit_made_since_the_last_load() {
        let path = config_file("live_hand_edit", CONFIG);
        let live = LiveConfig::load(&path).unwrap();
        let edited = format!("{CONFIG}session:\n  record: answer\n");
        crate::util::atomic::write_private(&path, edited.as_bytes()).unwrap();

        live.update(|config| config.set_tool_enabled("run_command", false))
            .unwrap();

        let saved = Config::load_from(&path).unwrap();
        assert!(!saved.tool_enabled("run_command"));
        assert!(!saved.thread().record.records_reasoning(), "hand edit kept");
    }

    #[test]
    fn a_broken_file_keeps_the_last_good_config() {
        let path = config_file("live_broken", CONFIG);
        let live = LiveConfig::load(&path).unwrap();
        crate::util::atomic::write_private(&path, b"providers: [not, a, map]\n").unwrap();

        assert!(live.reload().is_err());
        assert!(live.reload_if_changed().is_err());
        assert!(live.reload_if_changed().is_ok(), "reported once");
        assert_eq!(live.current().providers().len(), 1);
    }

    use crate::secrets::MemoryStore;

    /// A store that refuses to save, as a locked keychain does.
    struct Locked;

    impl SecretStore for Locked {
        fn get(&self, _: &str) -> Result<Option<String>> {
            Ok(None)
        }
        fn set(&self, _: &str, _: &str) -> Result<()> {
            anyhow::bail!("locked")
        }
        fn delete(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn keys_leave_the_file_for_the_store_and_stay_in_memory() {
        let path = config_file("live_keys_move", CONFIG);
        let store = Arc::new(MemoryStore::default());
        let live = LiveConfig::load_with(&path, Some(store.clone())).unwrap();

        assert_eq!(
            store.get("provider/main").unwrap().as_deref(),
            Some("sk-test")
        );
        assert!(!fs::read_to_string(&path).unwrap().contains("sk-test"));
        assert_eq!(live.current().providers()["main"].api_key.0, "sk-test");
        // A later start finds the key in the store, not in the file.
        let again = LiveConfig::load_with(&path, Some(store)).unwrap();
        assert_eq!(again.current().providers()["main"].api_key.0, "sk-test");
    }

    #[test]
    fn a_key_the_store_refuses_stays_in_the_file() {
        let path = config_file("live_keys_locked", CONFIG);
        let live = LiveConfig::load_with(&path, Some(Arc::new(Locked))).unwrap();

        assert!(fs::read_to_string(&path).unwrap().contains("sk-test"));
        assert_eq!(live.current().providers()["main"].api_key.0, "sk-test");
    }

    #[test]
    fn a_new_key_goes_to_the_store_and_a_removed_provider_takes_its_key_along() {
        let path = config_file("live_keys_update", CONFIG);
        let store = Arc::new(MemoryStore::default());
        let live = LiveConfig::load_with(&path, Some(store.clone())).unwrap();

        live.update(|config| {
            let mut other = config.providers()["main"].clone();
            other.api_key.0 = "sk-other".into();
            config.set_provider("other".into(), other);
        })
        .unwrap();
        assert_eq!(
            store.get("provider/other").unwrap().as_deref(),
            Some("sk-other")
        );
        assert!(!fs::read_to_string(&path).unwrap().contains("sk-other"));

        live.update(|config| {
            config.remove_provider("other");
        })
        .unwrap();
        assert_eq!(store.get("provider/other").unwrap(), None);
        assert_eq!(
            store.get("provider/main").unwrap().as_deref(),
            Some("sk-test")
        );
    }

    #[test]
    fn an_update_the_store_refuses_changes_nothing() {
        let path = config_file("live_keys_refused", CONFIG);
        let live = LiveConfig::load_with(&path, Some(Arc::new(Locked))).unwrap();
        let before = fs::read_to_string(&path).unwrap();

        let result = live.update(|config| {
            let mut other = config.providers()["main"].clone();
            other.api_key.0 = "sk-other".into();
            config.set_provider("other".into(), other);
        });

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert_eq!(live.current().providers().len(), 1);
    }

    #[test]
    fn a_change_to_other_settings_does_not_need_the_keychain() {
        let path = config_file("live_keys_unrelated", CONFIG);
        let live = LiveConfig::load_with(&path, Some(Arc::new(Locked))).unwrap();

        live.update(|config| config.set_tool_enabled("run_command", false))
            .unwrap();
        assert!(
            !Config::load_from(&path)
                .unwrap()
                .tool_enabled("run_command")
        );
    }

    #[test]
    fn a_provider_removed_by_hand_takes_its_key_along() {
        let path = config_file("live_keys_hand_removed", CONFIG);
        let store = Arc::new(MemoryStore::default());
        let live = LiveConfig::load_with(&path, Some(store.clone())).unwrap();
        assert!(store.get("provider/main").unwrap().is_some());

        crate::util::atomic::write_private(&path, b"session:\n  record: answer\n").unwrap();
        live.reload().unwrap();

        assert_eq!(store.get("provider/main").unwrap(), None);
        // A provider of the same name added later starts without the old key.
        let readded = CONFIG.replace("sk-test", "").replace("api_key: ''\n", "");
        crate::util::atomic::write_private(&path, readded.as_bytes()).unwrap();
        live.reload().unwrap();
        assert!(live.current().providers()["main"].api_key.0.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_leaves_the_keys_in_the_keychain() {
        use std::os::unix::fs::PermissionsExt;
        let path = config_file("live_keys_write_fails", CONFIG);
        let store = Arc::new(MemoryStore::default());
        let live = LiveConfig::load_with(&path, Some(store.clone())).unwrap();
        let folder = path.parent().unwrap();
        fs::set_permissions(folder, fs::Permissions::from_mode(0o500)).unwrap();
        // A user that may write anyway, such as root, cannot show the failure.
        if fs::write(folder.join("probe"), b"").is_ok() {
            fs::set_permissions(folder, fs::Permissions::from_mode(0o700)).unwrap();
            return;
        }

        let result = live.update(|config| {
            config.remove_provider("main");
        });
        fs::set_permissions(folder, fs::Permissions::from_mode(0o700)).unwrap();

        assert!(result.is_err());
        assert_eq!(
            store.get("provider/main").unwrap().as_deref(),
            Some("sk-test")
        );
    }
}
