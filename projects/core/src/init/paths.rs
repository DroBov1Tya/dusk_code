use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, ensure};

const CONFIG_ROOT: &str = ".config/duskcode";
const CONFIG_FILE: &str = "config.yml";
/// Where MAIN.md was before modes; moved into `modes/` at start.
const LEGACY_MAIN_PROMPT: &str = "MAIN.md";
const MODES_DIR: &str = "modes";
const COMPACT_PROMPT: &str = "COMPACT.md";
const MEMORY_FILE: &str = "MEMORY.md";
const FORK_PROMPT: &str = "FORK.md";
const SKILLS_DIR: &str = "skills";
const AGENTS_DIR: &str = "agents";
/// Holds the database; the name is the one the user chose.
const RECORDS_DIR: &str = "session";
const STATE_DB: &str = "state.db";
const WORKSPACE_ROOT: &str = "Documents/duskcode";
const MAX_FOLDER_NAME: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    config_root: PathBuf,
    workspace_root: PathBuf,
}

impl Paths {
    /// The standard layout under `$HOME`.
    pub fn from_home() -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
        let home = PathBuf::from(home);
        ensure!(home.is_absolute(), "HOME must be an absolute path");
        Ok(Self::new(home.join(CONFIG_ROOT), home.join(WORKSPACE_ROOT)))
    }

    /// The same layout under other roots, e.g. for tests.
    pub fn new(config_root: impl Into<PathBuf>, workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            config_root: config_root.into(),
            workspace_root: workspace_root.into(),
        }
    }

    pub fn config_root(&self) -> &Path {
        &self.config_root
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_root.join(CONFIG_FILE)
    }

    /// The prompt of every mode, one file each.
    pub fn modes_dir(&self) -> PathBuf {
        self.config_root.join(MODES_DIR)
    }

    /// MAIN mode's prompt, which the general agent works with too.
    pub fn main_prompt(&self) -> PathBuf {
        crate::catalog::modes::file(&self.modes_dir(), crate::catalog::modes::MAIN)
    }

    pub fn legacy_main_prompt(&self) -> PathBuf {
        self.config_root.join(LEGACY_MAIN_PROMPT)
    }

    /// How to summarize a thread when its context is compacted.
    pub fn compact_prompt(&self) -> PathBuf {
        self.config_root.join(COMPACT_PROMPT)
    }

    /// How to describe a thread for /fork, so a new one can go on from it.
    pub fn fork_prompt(&self) -> PathBuf {
        self.config_root.join(FORK_PROMPT)
    }

    /// What the agent remembers about its owner, written by the remember tool.
    pub fn memory_file(&self) -> PathBuf {
        self.config_root.join(MEMORY_FILE)
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.config_root.join("logs")
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.config_root.join(SKILLS_DIR)
    }

    pub fn agents_dir(&self) -> PathBuf {
        self.config_root.join(AGENTS_DIR)
    }

    pub fn records_dir(&self) -> PathBuf {
        self.config_root.join(RECORDS_DIR)
    }

    pub fn state_db(&self) -> PathBuf {
        self.records_dir().join(STATE_DB)
    }

    pub fn artifacts_dir(&self) -> PathBuf {
        self.records_dir().join("artifacts")
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Thread IDs are caller-assigned, so only plain folder names are accepted:
    /// nothing like `..` or `a/b` can lead outside the workspace root.
    pub fn thread_workspace(&self, thread_id: &str) -> Result<PathBuf> {
        ensure!(
            is_plain_folder_name(thread_id),
            "thread ID is not a valid folder name"
        );
        Ok(self.workspace_root.join(thread_id))
    }
}

pub fn is_plain_folder_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_FOLDER_NAME
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Paths {
        Paths::new(
            "/home/user/.config/duskcode",
            "/home/user/Documents/duskcode",
        )
    }

    #[test]
    fn thread_workspace_stays_inside_its_root() {
        let paths = paths();
        for name in ["", ".", "..", "../x", "a/b", "/etc", "a b", &"x".repeat(65)] {
            assert!(paths.thread_workspace(name).is_err(), "{name:?}");
        }
    }
}
