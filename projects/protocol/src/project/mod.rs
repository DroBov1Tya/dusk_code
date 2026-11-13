//! Projects: named folders the user works in, their git state, diffs and files.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

mod diff;
mod files;
mod git;
mod path;
mod schedule;

pub use diff::{DiffKind, DiffLine, DiffStat, FileDiff, Hunk};
pub use files::{
    FileEntry, FileStamp, FileText, MAX_EDIT_BYTES, MAX_PREVIEW_BYTES, OutputFile, OutputKind,
    OutputPreview,
};
pub use git::{Change, ChangeKind, GitInfo, Worktree};
pub use path::{PathCompletion, PathEntry};
pub use schedule::{
    Clock, DAY_NAMES, EVERY_DAY, MAX_INTERVAL_MINUTES, MAX_PROMPT_CHARS, MAX_SCHEDULES,
    MIN_INTERVAL_MINUTES, Repeat, Schedule, ScheduleDraft, WEEKDAYS, WEEKENDS, interval_name,
};
