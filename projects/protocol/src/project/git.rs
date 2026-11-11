//! What a client shows of a project's git state.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::project::DiffStat;
/// What a project's repository looks like now; `None` fields outside a repository.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitInfo {
    /// `None` on a detached head.
    pub branch: Option<String>,
    pub branches: Vec<String>,
    pub worktrees: Vec<Worktree>,
    /// Commits ahead of and behind the upstream, when there is one.
    pub ahead_behind: Option<(u64, u64)>,
    /// Paths relative to the project folder.
    pub changes: Vec<Change>,
    /// The repository's page, from the `origin` remote: https, no credentials.
    #[serde(default)]
    pub web: Option<String>,
    /// Lines added and removed in all changes against the last commit.
    #[serde(default)]
    pub stat: DiffStat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
}
