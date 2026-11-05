//! Pictures a tool or a message carries, as files.

use serde::{Deserialize, Serialize};

/// A durable card reference, carried separately from model-visible text. IDs are generated
/// locally; neither a URL nor a caller-supplied path is used as a storage filename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageArtifact {
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub byte_len: u64,
    pub sha256: String,
    /// The model sees it with the tool result that made it, such as a screenshot; other
    /// cards are for the user only.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub for_model: bool,
}
