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
    /// OpenAI on a ChatGPT plan: the Responses API, signed in to in the browser.
    #[serde(rename = "chatgpt")]
    ChatGpt,
    /// The Anthropic Messages API.
    #[serde(rename = "anthropic")]
    Anthropic,
    /// The Google AI Gemini API.
    #[serde(rename = "google")]
    Google,
}

impl ProviderKind {
    /// The provider's own API, for a form to fill in; none for a custom one.
    pub fn base_url(self) -> Option<&'static str> {
        match self {
            Self::OpenAiCompatible => None,
            Self::OpenAi | Self::ChatGpt => Some("https://api.openai.com/v1"),
            Self::Anthropic => Some("https://api.anthropic.com/v1"),
            Self::Google => Some("https://generativelanguage.googleapis.com/v1beta"),
        }
    }
}
