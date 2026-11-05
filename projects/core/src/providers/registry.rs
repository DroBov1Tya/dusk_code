//! Configured providers: built from the config by name, each dispatching to its protocol.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

use anyhow::Result;

use super::{
    anthropic::{Anthropic, MessageStream},
    openai::{CompletionStream, OpenAiCompatible},
    output::{ChatStream, ModelEvent},
    request::ChatRequest,
};
use crate::{
    BoxFuture,
    config::{Config, ProviderConfig, ProviderKind},
    providers::output::inline_tags::TagSplitter,
};

pub use dusk_protocol::providers::Model;

/// What the agent needs from a model provider. Tests put a scripted model behind it.
pub trait ModelProvider: Send + Sync {
    /// Returns the catalogue sorted by model ID.
    fn list_models(&self) -> BoxFuture<'_, Vec<Model>>;

    fn stream_chat<'a>(&'a self, request: ChatRequest<'a>) -> BoxFuture<'a, Box<dyn ChatStream>>;
}

/// Providers by config name, in name order.
pub struct ProviderRegistry {
    providers: BTreeMap<String, Arc<Provider>>,
}

pub struct Provider {
    adapter: ProviderAdapter,
    inline_tags: bool,
}

enum ProviderAdapter {
    OpenAiCompatible(OpenAiCompatible),
    Anthropic(Anthropic),
}

struct ProviderStream {
    adapter: StreamAdapter,
    tags: Option<TagSplitter>,
    pending: VecDeque<ModelEvent>,
}

enum StreamAdapter {
    OpenAiCompatible(CompletionStream),
    Anthropic(MessageStream),
}

impl ProviderRegistry {
    /// Builds adapters without making any network requests.
    pub fn from_config(config: &Config) -> Result<Self> {
        let mut providers = BTreeMap::new();
        for (name, settings) in &config.all_providers() {
            providers.insert(name.clone(), Arc::new(Provider::new(settings)?));
        }
        Ok(Self { providers })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.providers.keys().map(String::as_str)
    }

    pub fn provider(&self, name: &str) -> Option<Arc<Provider>> {
        self.providers.get(name).cloned()
    }
}

/// Validates endpoint security and optional credentials without a network request.
pub fn validate_provider(settings: &ProviderConfig) -> Result<()> {
    Provider::new(settings).map(drop)
}

impl Provider {
    pub fn new(settings: &ProviderConfig) -> Result<Self> {
        let adapter = match settings.kind {
            ProviderKind::OpenAiCompatible | ProviderKind::OpenAi => {
                ProviderAdapter::OpenAiCompatible(OpenAiCompatible::new(settings)?)
            }
            ProviderKind::Anthropic => ProviderAdapter::Anthropic(Anthropic::new(settings)?),
        };
        Ok(Self {
            adapter,
            inline_tags: settings.inline_tags,
        })
    }
}

impl ModelProvider for Provider {
    fn list_models(&self) -> BoxFuture<'_, Vec<Model>> {
        Box::pin(async move {
            match &self.adapter {
                ProviderAdapter::OpenAiCompatible(adapter) => adapter.list_models().await,
                ProviderAdapter::Anthropic(adapter) => adapter.list_models().await,
            }
        })
    }

    fn stream_chat<'a>(&'a self, request: ChatRequest<'a>) -> BoxFuture<'a, Box<dyn ChatStream>> {
        Box::pin(async move {
            let adapter = match &self.adapter {
                ProviderAdapter::OpenAiCompatible(adapter) => {
                    StreamAdapter::OpenAiCompatible(adapter.stream_chat(request).await?)
                }
                ProviderAdapter::Anthropic(adapter) => {
                    StreamAdapter::Anthropic(adapter.stream_chat(request).await?)
                }
            };
            let stream = ProviderStream {
                adapter,
                tags: self.inline_tags.then(TagSplitter::default),
                pending: VecDeque::new(),
            };
            Ok(Box::new(stream) as Box<dyn ChatStream>)
        })
    }
}

impl ChatStream for ProviderStream {
    fn next_event(&mut self) -> BoxFuture<'_, Option<ModelEvent>> {
        Box::pin(async move {
            loop {
                if let Some(event) = self.pending.pop_front() {
                    return Ok(Some(event));
                }
                let event = match &mut self.adapter {
                    StreamAdapter::OpenAiCompatible(stream) => stream.next_event().await?,
                    StreamAdapter::Anthropic(stream) => stream.next_event().await?,
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
        })
    }
}
