//! The models a provider offers.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A configured provider as a check found it: its models, or why it did not answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub name: String,
    pub models: Result<Vec<Model>, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    /// Maximum input tokens, when the provider's catalogue reports it.
    pub context_window: Option<u64>,
    /// Maximum output tokens of one call, when the catalogue reports it.
    pub max_output: Option<u64>,
}
