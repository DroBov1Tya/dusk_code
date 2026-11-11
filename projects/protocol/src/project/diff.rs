//! A diff, file by file.

use serde::{Deserialize, Serialize};

use crate::project::ChangeKind;
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
