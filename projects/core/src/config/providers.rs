//! Model providers as the config lists them; the API key is never shown.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

use super::sections::{is_false, is_true, yes};

pub use dusk_protocol::config::ProviderKind;

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
    /// Plain HTTP to a host outside this machine and the private networks; the key
    /// then travels unencrypted, so it is never on by default.
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_http: bool,
    /// Prices by model ID, for providers that do not report the cost of a call.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub prices: BTreeMap<String, Price>,
    /// Where the provider tells the balance; known hosts need no entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<BalanceApi>,
    /// At most this many model calls a minute, after a burst of as many; no limit when
    /// not set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests_per_minute: Option<u32>,
    /// Off keeps the provider and its key but never asks it or offers its models.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

/// US dollars per million tokens, as the provider's price list gives them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Price {
    pub input: f64,
    /// Input served from the cache; the plain input price when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input: Option<f64>,
    /// Input written to the cache; the plain input price when not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
    pub output: f64,
}

/// The balance endpoints a plain API key may read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BalanceApi {
    Deepseek,
    Kimi,
    Openrouter,
    Litellm,
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
