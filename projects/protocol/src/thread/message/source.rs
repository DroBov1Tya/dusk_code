//! A file kept among a thread's sources.

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub source_id: String,
    pub name: String,
    pub file: String,
    pub bytes: u64,
    pub pinned: bool,
    pub delivered: bool,
}

/// The most bytes of a source one `SourceText` answer carries; a reader asks again from
/// `next` for more.
pub const SOURCE_CHUNK: u64 = 64 * 1024;

/// What a source holds, as the core told it by its bytes.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    /// UTF-8 text; `SourceChunk::text` holds a part of it.
    Text,
    /// A PNG or JPEG; its bytes come with `SourcePicture`.
    Picture,
    /// Anything else, such as a PDF; no text is given out.
    Binary,
}

/// A part of a source for a client to read: at most `SOURCE_CHUNK` bytes from `offset`,
/// cut at a character's edge.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceChunk {
    pub kind: SourceKind,
    /// Empty for a picture or a binary file.
    pub text: String,
    pub offset: u64,
    /// Where the next part starts; none at the end.
    pub next: Option<u64>,
    /// The file's whole length.
    pub bytes: u64,
    /// The file on the core's machine, for a client there to open in its own app; empty
    /// for a client elsewhere.
    pub path: String,
}
