//! The kinds of provider.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    #[default]
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
    /// OpenAI itself: Chat Completions with `max_completion_tokens`.
    #[serde(rename = "openai")]
    OpenAi,
    /// The Anthropic Messages API.
    #[serde(rename = "anthropic")]
    Anthropic,
}

impl ProviderKind {
    /// The provider's own API, for a form to fill in; none for a custom one.
    pub fn base_url(self) -> Option<&'static str> {
        match self {
            Self::OpenAiCompatible => None,
            Self::OpenAi => Some("https://api.openai.com/v1"),
            Self::Anthropic => Some("https://api.anthropic.com/v1"),
        }
    }
}
