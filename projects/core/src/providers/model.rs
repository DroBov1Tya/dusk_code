//! Model identity and catalogue discovery.

use std::future::Future;

use anyhow::Result;

/// A model offered by a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// Provider-specific model ID, sent with every request.
    pub id: String,
}

/// Lists the models a provider offers.
pub trait ModelProvider: Send + Sync {
    /// Returns the catalogue sorted by model ID.
    fn list_models(&self) -> impl Future<Output = Result<Vec<Model>>> + Send;
}
