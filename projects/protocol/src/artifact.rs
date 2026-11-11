//! Cards a tool or a message carries, as files: pictures and sounds.

use serde::{Deserialize, Serialize};

/// A durable card reference, carried separately from model-visible text. IDs are generated
/// locally; neither a URL nor a caller-supplied path is used as a storage filename.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
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
    /// What the card holds. Records written before sounds existed read as pictures.
    #[serde(default, skip_serializing_if = "CardKind::is_image")]
    pub kind: CardKind,
    /// How long a sound plays, in milliseconds; zero when unknown, and for pictures.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardKind {
    #[default]
    Image,
    Audio,
}

impl CardKind {
    fn is_image(&self) -> bool {
        *self == Self::Image
    }
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}
