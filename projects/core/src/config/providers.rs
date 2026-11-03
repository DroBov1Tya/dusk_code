//! Model providers as the config lists them; the API key is never shown.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::sections::is_false;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    #[serde(default)]
    pub kind: ProviderKind,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "ApiKey::is_empty")]
    pub api_key: ApiKey,
    /// The server leaves `<think>` and `<tool_call>` tags inside the answer text.
    #[serde(default, skip_serializing_if = "is_false")]
    pub inline_tags: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApiKey(pub String);

impl ApiKey {
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for ProviderConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderConfig")
            .field("kind", &self.kind)
            .field("base_url", &"[REDACTED]")
            .field("api_key", &self.api_key)
            .finish()
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}
