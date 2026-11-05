//! Release channels, what the newest release is, and how far an install got.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Dev,
}

impl Channel {
    pub fn name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Dev => "dev",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Stable => Self::Dev,
            Self::Dev => Self::Stable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current: String,
    pub channel: Channel,
    /// The newest version on the channel, as its tag.
    pub latest: String,
    pub prerelease: bool,
    /// Commits from the running version to a newer `latest`, when GitHub knows both.
    pub commits_behind: Option<u64>,
    /// `latest` is newer than the running version.
    pub newer: bool,
    /// The release has the archive for this target; right after a tag it may still be
    /// building on GitHub.
    pub ready: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallStage {
    Preparing,
    Downloading,
    Verifying,
    Installing,
    Complete,
}

/// Download byte counts stay available through verification and installation.
/// A missing or zero content length does not give a useful percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallProgress {
    pub stage: InstallStage,
    pub downloaded: u64,
    pub total: Option<u64>,
}

impl Default for InstallProgress {
    fn default() -> Self {
        Self {
            stage: InstallStage::Preparing,
            downloaded: 0,
            total: None,
        }
    }
}
