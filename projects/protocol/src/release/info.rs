//! The newest release against the running one.

use serde::{Deserialize, Serialize};

use crate::release::Channel;
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
