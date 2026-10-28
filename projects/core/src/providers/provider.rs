//! Configured providers: built from the config by name, each dispatching to its protocol.

use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
};

use anyhow::{Context, Result};

use super::{
    chat::ChatRequest,
    inline_tags::TagSplitter,
    model_output::{ChatStream, ModelEvent},
    openai::{CompletionStream, OpenAiCompatible},
};
use crate::config::{Config, ProviderKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    /// Maximum input tokens, when the provider's catalogue reports it.
    pub context_window: Option<u64>,
}

/// What the agent needs from a model provider.
pub trait ModelProvider: Send + Sync {
    type Stream: ChatStream;

    /// Returns the catalogue sorted by model ID.
    fn list_models(&self) -> impl Future<Output = Result<Vec<Model>>> + Send;

    fn stream_chat(
        &self,
        request: ChatRequest<'_>,
    ) -> impl Future<Output = Result<Self::Stream>> + Send;
}

/// Providers by config name, in name order.
pub struct ProviderRegistry {
    providers: BTreeMap<String, Provider>,
}

pub struct Provider {
    adapter: ProviderAdapter,
    inline_tags: bool,
}

enum ProviderAdapter {
    OpenAiCompatible(OpenAiCompatible),
}

pub struct ProviderStream {
    adapter: StreamAdapter,
    tags: Option<TagSplitter>,
    pending: VecDeque<ModelEvent>,
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
            let provider = Provider {
                adapter,
                inline_tags: settings.inline_tags,
            };
            providers.insert(name.clone(), provider);
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
}

impl ModelProvider for Provider {
    type Stream = ProviderStream;

    async fn list_models(&self) -> Result<Vec<Model>> {
        match &self.adapter {
            ProviderAdapter::OpenAiCompatible(adapter) => adapter.list_models().await,
        }
    }

    async fn stream_chat(&self, request: ChatRequest<'_>) -> Result<ProviderStream> {
        let adapter = match &self.adapter {
            ProviderAdapter::OpenAiCompatible(adapter) => {
                StreamAdapter::OpenAiCompatible(adapter.stream_chat(request).await?)
            }
        };
        Ok(ProviderStream {
            adapter,
            tags: self.inline_tags.then(TagSplitter::default),
            pending: VecDeque::new(),
        })
    }
}

impl ChatStream for ProviderStream {
    async fn next_event(&mut self) -> Result<Option<ModelEvent>> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
            }
            let event = match &mut self.adapter {
                StreamAdapter::OpenAiCompatible(stream) => stream.next_event().await?,
            };
            let Some(tags) = &mut self.tags else {
                return Ok(event);
            };
            match event {
                Some(ModelEvent::Text(text)) => self.pending.extend(tags.feed(&text)?),
                Some(finished @ ModelEvent::Finished(_)) => {
                    self.pending.extend(tags.finish()?);
                    self.pending.push_back(finished);
                }
                other => return Ok(other),
            }
        }
    }
}
