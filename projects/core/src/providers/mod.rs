//! Provider-independent model discovery and streamed chat for host adapters.

mod openai;
mod sse;

use std::{collections::BTreeMap, future::Future};

use anyhow::{Context, Result};

use crate::config::{Config, ProviderKind};
use openai::{CompletionStream, OpenAiCompatible};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub id: String,
}

pub trait ModelProvider: Send + Sync {
    fn list_models(&self) -> impl Future<Output = Result<Vec<Model>>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
}

/// Exactly what the model sees: a role and text, without core metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

/// One increment of a streamed answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatDelta {
    /// Model thinking: shown to the user, never stored as the answer.
    Reasoning(String),
    Text(String),
}

pub trait ChatStream: Send {
    /// Returns `None` once the model has finished; an error ends the stream.
    fn next_delta(&mut self) -> impl Future<Output = Result<Option<ChatDelta>>> + Send;
}

pub struct ProviderStream {
    adapter: StreamAdapter,
}

enum StreamAdapter {
    OpenAiCompatible(CompletionStream),
}

pub struct ProviderRegistry {
    providers: BTreeMap<String, Provider>,
}

pub struct Provider {
    adapter: ProviderAdapter,
}

enum ProviderAdapter {
    OpenAiCompatible(OpenAiCompatible),
}

impl ProviderRegistry {
    /// Build adapters without making any network requests.
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

impl ChatStream for ProviderStream {
    async fn next_delta(&mut self) -> Result<Option<ChatDelta>> {
        match &mut self.adapter {
            StreamAdapter::OpenAiCompatible(stream) => stream.next_delta().await,
        }
    }
}

impl ModelProvider for Provider {
    async fn list_models(&self) -> Result<Vec<Model>> {
        match &self.adapter {
            ProviderAdapter::OpenAiCompatible(adapter) => adapter.list_models().await,
        }
    }
}
