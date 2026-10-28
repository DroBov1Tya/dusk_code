//! Every file and directory the application uses, in one place:
//!
//! ```text
//! ~/.config/duskcode/            configuration, mode 0700
//!     config.yml                 provider and session settings, mode 0600
//!     MAIN.md                    system prompt of the main agent, mode 0600
//!     skills/<name>/SKILL.md     skills, mode 0700
//!     agents/<name>.md           system prompts of subagent kinds, mode 0700
//!     session/                   session records, mode 0700
//!         state.db               threads, turns and messages, mode 0600
//! ~/Documents/duskcode/          session workspaces, mode 0700
//!     <thread id>/               one per session: temporary and downloaded files
//! ```
//!
//! The same layout is used on every platform; `XDG_CONFIG_HOME` is deliberately ignored.

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, ensure};

const CONFIG_ROOT: &str = ".config/duskcode";
const CONFIG_FILE: &str = "config.yml";
const MAIN_PROMPT: &str = "MAIN.md";
const SKILLS_DIR: &str = "skills";
const AGENTS_DIR: &str = "agents";
const SESSION_DIR: &str = "session";
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

    pub fn main_prompt(&self) -> PathBuf {
        self.config_root.join(MAIN_PROMPT)
    }

    pub fn skills_dir(&self) -> PathBuf {
        self.config_root.join(SKILLS_DIR)
    }

    pub fn agents_dir(&self) -> PathBuf {
        self.config_root.join(AGENTS_DIR)
    }

    pub fn session_dir(&self) -> PathBuf {
        self.config_root.join(SESSION_DIR)
    }

    pub fn state_db(&self) -> PathBuf {
        self.session_dir().join(STATE_DB)
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Thread IDs are caller-assigned, so only plain folder names are accepted:
    /// nothing like `..` or `a/b` can lead outside the workspace root.
    pub fn session_workspace(&self, thread_id: &str) -> Result<PathBuf> {
        ensure!(
            is_plain_folder_name(thread_id),
            "thread ID is not a valid folder name"
        );
        Ok(self.workspace_root.join(thread_id))
    }
}

fn is_plain_folder_name(name: &str) -> bool {
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
    fn layout_has_two_roots() {
        let paths = paths();
        assert_eq!(
            paths.config_file(),
            Path::new("/home/user/.config/duskcode/config.yml")
        );
        assert_eq!(
            paths.state_db(),
            Path::new("/home/user/.config/duskcode/session/state.db")
        );
        assert_eq!(
            paths.session_workspace("0199-abc").unwrap(),
            Path::new("/home/user/Documents/duskcode/0199-abc")
        );
    }

    #[test]
    fn session_workspace_stays_inside_its_root() {
        let paths = paths();
        for name in ["", ".", "..", "../x", "a/b", "/etc", "a b", &"x".repeat(65)] {
            assert!(paths.session_workspace(name).is_err(), "{name:?}");
        }
    }
}
