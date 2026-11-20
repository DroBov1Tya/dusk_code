//! The change ledger: what a thread's commands changed in its project, file by file,
//! kept without git.

use serde::{Deserialize, Serialize};

/// The most rows `WorkRequest::FileChanges` answers with, newest first.
pub const MAX_FILE_CHANGES: usize = 200;

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditKind {
    Created,
    Modified,
    Deleted,
}

/// One file one tool call changed.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub change_id: String,
    pub thread_id: String,
    /// The turn the call ran in; `None` when it ran outside one.
    pub turn_id: Option<String>,
    /// The turn's number in its thread, from 1.
    pub turn: Option<u32>,
    pub call_id: Option<String>,
    /// Relative to the project folder, with `/` between parts.
    pub path: String,
    pub kind: EditKind,
    /// `None` for a file that did not exist before (created) or after (deleted).
    pub size_before: Option<u64>,
    pub size_after: Option<u64>,
    /// SHA-256 of the contents after, in hex, when the file was small enough to read.
    pub hash_after: Option<String>,
    /// A unified diff, when the text before was known and the file is no secret.
    pub diff: Option<String>,
    /// The diff was cut to its cap; the text ends with a note saying so.
    pub diff_cut: bool,
    /// Unix timestamp in milliseconds.
    pub at: i64,
}
