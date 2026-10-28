//! Every file and directory the application uses, in one place:
//!
//! ```text
//! ~/.config/duskcode/            root, mode 0700
//!     config.yml                 provider settings, mode 0600
//!     session/                   session data, mode 0700
//!         state.db               threads, turns and messages
//! ```
//!
//! The same layout is used on every platform; `XDG_CONFIG_HOME` is deliberately ignored.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, ensure};

const ROOT_DIR: &str = ".config/duskcode";
const CONFIG_FILE: &str = "config.yml";
const SESSION_DIR: &str = "session";
const STATE_DB: &str = "state.db";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    /// The standard layout under `$HOME`.
    pub fn from_home() -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
        let home = PathBuf::from(home);
        ensure!(home.is_absolute(), "HOME must be an absolute path");
        Ok(Self::new(home.join(ROOT_DIR)))
    }

    /// The same layout under another root, e.g. for tests.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join(CONFIG_FILE)
    }

    pub fn session_dir(&self) -> PathBuf {
        self.root.join(SESSION_DIR)
    }

    pub fn state_db(&self) -> PathBuf {
        self.session_dir().join(STATE_DB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_rooted_in_one_directory() {
        let paths = Paths::new("/home/user/.config/duskcode");
        assert_eq!(
            paths.config_file(),
            Path::new("/home/user/.config/duskcode/config.yml")
        );
        assert_eq!(
            paths.state_db(),
            Path::new("/home/user/.config/duskcode/session/state.db")
        );
    }
}
