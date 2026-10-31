//! Configured providers: built from the config by name, each dispatching to its protocol.

use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    sync::Arc,
};

use anyhow::{Context, Result};

use super::{
    chat::ChatRequest,
    model_output::{ChatStream, ModelEvent},
    openai::{CompletionStream, OpenAiCompatible},
};
use crate::{
    BoxFuture,
    chat::inline_tags::TagSplitter,
    config::{Config, ProviderConfig, ProviderKind},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub id: String,
    /// Maximum input tokens, when the provider's catalogue reports it.
    pub context_window: Option<u64>,
}

/// A failure that may pass if the same request is sent again: the network, a busy or
/// failing server, a cut stream.
#[derive(Debug)]
pub struct Transient(pub String);

/// The provider explicitly rejected `reasoning_effort: none`. This is permanent for
/// this request and must never trigger a resend with a different effort.
#[derive(Debug)]
pub struct UnsupportedEffort;

impl fmt::Display for UnsupportedEffort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .write_str("provider does not support reasoning effort off; request was not resent")
    }
}

impl std::error::Error for UnsupportedEffort {}

pub fn is_unsupported_effort(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<UnsupportedEffort>().is_some())
}

/// The provider refused a request that carried pictures. It may say nothing about why,
/// so the same request without them tells.
#[derive(Debug)]
pub struct UnsupportedPictures;

impl fmt::Display for UnsupportedPictures {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("provider refused a request with pictures")
    }
}

impl std::error::Error for UnsupportedPictures {}

pub fn is_unsupported_pictures(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<UnsupportedPictures>().is_some())
}

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
}

struct ProviderStream {
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
    match settings.kind {
        ProviderKind::OpenAiCompatible => {
            OpenAiCompatible::new(settings)?;
        }
    }
    Ok(())
}

impl Provider {
    pub fn new(settings: &ProviderConfig) -> Result<Self> {
        let adapter = match settings.kind {
            ProviderKind::OpenAiCompatible => {
                ProviderAdapter::OpenAiCompatible(OpenAiCompatible::new(settings)?)
            }
        };
        Ok(Self {
            adapter,
            inline_tags: settings.inline_tags,
        })
    }

    /// Picks the first model of the catalogue so callers need no configured model ID.
    pub async fn default_model(&self) -> Result<Model> {
        self.list_models()
            .await?
            .into_iter()
            .next()
            .context("provider offers no models")
    }
}

impl fmt::Display for Transient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Transient {}

/// Whether `error` or anything it wraps is a `Transient` failure.
pub fn is_transient(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<Transient>().is_some())
}

impl ModelProvider for Provider {
    fn list_models(&self) -> BoxFuture<'_, Vec<Model>> {
        Box::pin(async move {
            match &self.adapter {
                ProviderAdapter::OpenAiCompatible(adapter) => adapter.list_models().await,
            }
        })
    }

    fn stream_chat<'a>(&'a self, request: ChatRequest<'a>) -> BoxFuture<'a, Box<dyn ChatStream>> {
        Box::pin(async move {
            let adapter = match &self.adapter {
                ProviderAdapter::OpenAiCompatible(adapter) => {
                    StreamAdapter::OpenAiCompatible(adapter.stream_chat(request).await?)
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
