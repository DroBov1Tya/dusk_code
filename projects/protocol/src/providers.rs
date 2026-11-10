//! The models a provider offers.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A configured provider as a check found it: its models, or why it did not answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub name: String,
    pub models: Result<Vec<Model>, String>,
    /// What is left on the account; none when the provider does not tell it.
    #[serde(default)]
    pub balance: Option<Result<Balance, String>>,
}

/// Money left on a provider account, as the provider wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Balance {
    /// A decimal number; none when the key has no spending limit.
    pub left: Option<String>,
    /// Such as `USD` or `CNY`.
    pub currency: String,
}

impl Balance {
    pub fn text(&self) -> String {
        match &self.left {
            Some(left) => format!("{left} {}", self.currency),
            None => "no limit".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    /// Maximum input tokens, when the provider's catalogue reports it.
    pub context_window: Option<u64>,
    /// Maximum output tokens of one call, when the catalogue reports it.
    pub max_output: Option<u64>,
}
