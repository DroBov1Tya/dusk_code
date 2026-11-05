//! Projects: a named folder the user works in, and what a client shows of it: its git
//! state and its files. Chats belong to a project by the folder they started in.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The longest name and description a project may have, in characters.
pub const MAX_NAME_CHARS: usize = 80;
pub const MAX_DESCRIPTION_CHARS: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Absolute and resolved, with no symbolic links.
    pub dir: PathBuf,
    /// Unix timestamp in milliseconds.
    pub created_at: i64,
}

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

/// One entry of a project folder; hidden and ignored files are left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Relative to the project folder, with `/` between parts.
    pub path: String,
    pub is_dir: bool,
    /// For a folder, set when anything inside it changed.
    pub change: Option<ChangeKind>,
}
