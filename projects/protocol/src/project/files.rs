//! Files of a project and what the agents wrote.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::project::ChangeKind;
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

/// How to preview an output image without opening the original.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputPreview {
    #[default]
    Original,
    Thumbnail(PathBuf),
    Unavailable,
}

/// A file the agents made or keep: under the workspace, the pictures folder or a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputFile {
    /// Absolute and resolved.
    pub path: PathBuf,
    pub kind: OutputKind,
    #[serde(default)]
    pub preview: OutputPreview,
    pub bytes: u64,
    /// Unix timestamp in milliseconds.
    pub modified: i64,
}

/// The most a text file may weigh to be read for a preview.
pub const MAX_PREVIEW_BYTES: u64 = 1024 * 1024;

/// The most a text file may weigh to be opened in the editor or written back by it.
pub const MAX_EDIT_BYTES: u64 = 8 * 1024 * 1024;

/// What a file was on disk at one moment. A client sends back the stamp it read with, so
/// a file that something else changed meanwhile is not written over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStamp {
    /// Unix timestamp in milliseconds.
    pub modified_ms: i64,
    pub bytes: u64,
}

/// A text file as the editor opens it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileText {
    pub text: String,
    pub stamp: FileStamp,
}
