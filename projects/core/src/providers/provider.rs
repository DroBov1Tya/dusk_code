//! Configured providers: built from the config by name, each dispatching to its protocol.

use std::{collections::BTreeMap, future::Future};

use anyhow::{Context, Result};

use super::{
    chat::{ChatDelta, ChatMessage, ChatStream},
    openai::{CompletionStream, OpenAiCompatible},
};
use crate::config::{Config, ProviderKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub id: String,
}

pub trait ModelProvider: Send + Sync {
    /// Returns the catalogue sorted by model ID.
    fn list_models(&self) -> impl Future<Output = Result<Vec<Model>>> + Send;
}

/// Providers by config name, in name order.
pub struct ProviderRegistry {
    providers: BTreeMap<String, Provider>,
}

pub struct Provider {
    adapter: ProviderAdapter,
}

enum ProviderAdapter {
    OpenAiCompatible(OpenAiCompatible),
}

pub struct ProviderStream {
    adapter: StreamAdapter,
}

enum StreamAdapter {
    OpenAiCompatible(CompletionStream),
}

impl ProviderRegistry {
    /// Builds adapters without making any network requests.
    pub fn from_config(config: &Config) -> Result<Self> {
        let mut providers = BTreeMap::new();
        for (name, settings) in config.providers() {
            let adapter = match settings.kind {
                ProviderKind::OpenAiCompatible => {
                    ProviderAdapter::OpenAiCompatible(OpenAiCompatible::new(settings)?)
                }
            };
            providers.insert(name.clone(), Provider { adapter });
        }
        Ok(Self { providers })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.providers.keys().map(String::as_str)
    }

    pub fn provider(&self, name: &str) -> Option<&Provider> {
        self.providers.get(name)
    }
}

impl Provider {
    /// Picks the first model of the catalogue so callers need no configured model ID.
    pub async fn default_model(&self) -> Result<Model> {
        self.list_models()
            .await?
            .into_iter()
            .next()
            .context("provider offers no models")
    }

    pub async fn stream_chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
    ) -> Result<ProviderStream> {
        let adapter = match &self.adapter {
            ProviderAdapter::OpenAiCompatible(adapter) => {
                StreamAdapter::OpenAiCompatible(adapter.stream_chat(model, messages).await?)
            }
        };
        Ok(ProviderStream { adapter })
    }
}

impl ModelProvider for Provider {
    async fn list_models(&self) -> Result<Vec<Model>> {
        match &self.adapter {
            ProviderAdapter::OpenAiCompatible(adapter) => adapter.list_models().await,
        }
    }
}

impl ChatStream for ProviderStream {
    async fn next_delta(&mut self) -> Result<Option<ChatDelta>> {
        match &mut self.adapter {
            StreamAdapter::OpenAiCompatible(stream) => stream.next_delta().await,
        }
    }
}
