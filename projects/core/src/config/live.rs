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
use tracing::warn;

use crate::config::Config;

pub struct LiveConfig {
    path: PathBuf,
    current: RwLock<Arc<Config>>,
    /// Serializes reloads and writes, and remembers which file version is loaded.
    loaded: Mutex<FileStamp>,
}

/// Enough to notice that the file changed without reading it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: SystemTime,
    len: u64,
}

impl LiveConfig {
    pub fn load(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let stamp = FileStamp::of(&path)?;
        let config = Config::load_from(&path)?;
        Ok(Self {
            path,
            current: RwLock::new(Arc::new(config)),
            loaded: Mutex::new(stamp),
        })
    }

    /// A snapshot: later changes do not affect it.
    pub fn current(&self) -> Arc<Config> {
        Arc::clone(&self.current.read())
    }

    /// Reads the file now, for example after something was added through a client.
    pub fn reload(&self) -> Result<()> {
        let mut loaded = self.lock_loaded();
        self.read_from_disk(&mut loaded)
    }

    /// Changes the config on disk and in memory. The file is read first, so a hand edit
    /// made since the last reload is kept rather than overwritten.
    pub fn update(&self, change: impl FnOnce(&mut Config)) -> Result<Arc<Config>> {
        let mut loaded = self.lock_loaded();
        self.read_from_disk(&mut loaded)?;
        let mut config = Config::clone(&self.current());
        change(&mut config);
        let yaml = config.to_yaml()?;
        // Parsing what will be written proves the file will load again.
        Config::parse(yaml.as_bytes())?;
        crate::util::atomic::write_private(&self.path, yaml.as_bytes())?;
        *loaded = FileStamp::of(&self.path)?;
        let config = Arc::new(config);
        *self.current.write() = Arc::clone(&config);
        Ok(config)
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
        self.read_from_disk(&mut loaded)
    }

    fn read_from_disk(&self, loaded: &mut FileStamp) -> Result<()> {
        let stamp = FileStamp::of(&self.path)?;
        let config = Config::load_from(&self.path)?;
        *self.current.write() = Arc::new(config);
        *loaded = stamp;
        Ok(())
    }

    fn lock_loaded(&self) -> MutexGuard<'_, FileStamp> {
        // The guarded value is a plain stamp, still valid after a panic elsewhere.
        self.loaded.lock()
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
}
