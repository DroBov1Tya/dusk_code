//! Projects: a named folder the user works in, and what a client shows of it: its git
//! state and its files. Chats belong to a project by the folder they started in.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The longest name and description a project may have, in characters.
pub const MAX_NAME_CHARS: usize = 80;
pub const MAX_DESCRIPTION_CHARS: usize = 2000;
/// What a project's environment variables may hold.
pub const MAX_ENV_VARS: usize = 64;
pub const MAX_ENV_NAME_CHARS: usize = 64;
pub const MAX_ENV_VALUE_BYTES: usize = 8 * 1024;

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
    /// The repository's page, from the `origin` remote: https, no credentials.
    #[serde(default)]
    pub web: Option<String>,
    /// Lines added and removed in all changes against the last commit.
    #[serde(default)]
    pub stat: DiffStat,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffStat {
    pub files: u64,
    pub added: u64,
    pub removed: u64,
}

/// One changed file as a reader of a diff sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    /// Relative to the project folder.
    pub path: String,
    pub kind: ChangeKind,
    pub added: u64,
    pub removed: u64,
    /// Contents that are not text, or too large to show: no hunks.
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@` line, with the function git names after it.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffKind,
    /// Line numbers in the old and the new file; `None` on the side the line is not in.
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffKind {
    Context,
    Added,
    Removed,
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

/// What kind of file an output is, as the artifacts page groups them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputKind {
    Text,
    Image,
    Document,
    Other,
}

/// A file the agents made or keep: under the workspace, the pictures folder or a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputFile {
    /// Absolute and resolved.
    pub path: PathBuf,
    pub kind: OutputKind,
    pub bytes: u64,
    /// Unix timestamp in milliseconds.
    pub modified: i64,
}

/// The most a text file may weigh to be read for a preview.
pub const MAX_PREVIEW_BYTES: u64 = 1024 * 1024;
