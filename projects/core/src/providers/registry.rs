//! Configured providers: built from the config by name, each dispatching to its protocol.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

use anyhow::Result;

use super::{
    anthropic::{Anthropic, MessageStream},
    google::{GenerateStream, Google},
    openai::{ChatGpt, CompletionStream, OpenAiCompatible, ResponseStream},
    output::{ChatStream, ModelEvent},
    request::ChatRequest,
};
use crate::{
    BoxFuture,
    config::{Config, Price, ProviderConfig, ProviderKind},
    providers::output::inline_tags::TagSplitter,
};

pub use dusk_protocol::model::Model;

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
    prices: BTreeMap<String, Price>,
    rate: Option<super::rate::TokenBucket>,
    account: super::limits::Account,
}

enum ProviderAdapter {
    OpenAiCompatible(OpenAiCompatible),
    ChatGpt(ChatGpt),
    Anthropic(Anthropic),
    Google(Google),
}

struct ProviderStream {
    adapter: StreamAdapter,
    tags: Option<TagSplitter>,
    pending: VecDeque<ModelEvent>,
    /// The model's price, for a usage report that comes without a cost.
    price: Option<Price>,
}

enum StreamAdapter {
    OpenAiCompatible(CompletionStream),
    ChatGpt(ResponseStream),
    Anthropic(MessageStream),
    Google(GenerateStream),
}

impl ProviderRegistry {
    /// Builds adapters without making any network requests. A provider switched off is
    /// left out, so nothing can ask it, choose it or discover a model on it.
    pub fn from_config(config: &Config) -> Result<Self> {
        let mut providers = BTreeMap::new();
        for (name, settings) in &config.all_providers() {
            if !settings.enabled {
                continue;
            }
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
        for (model, price) in &settings.prices {
            super::cost::check(model, price)?;
        }
        anyhow::ensure!(
            settings
                .requests_per_minute
                .is_none_or(|limit| (1..=100_000).contains(&limit)),
            "requests_per_minute must be from 1 to 100000"
        );
        let adapter = match settings.kind {
            ProviderKind::OpenAiCompatible | ProviderKind::OpenAi => {
                ProviderAdapter::OpenAiCompatible(OpenAiCompatible::new(settings)?)
            }
            ProviderKind::ChatGpt => ProviderAdapter::ChatGpt(ChatGpt::new(settings)?),
            ProviderKind::Anthropic => ProviderAdapter::Anthropic(Anthropic::new(settings)?),
            ProviderKind::Google => ProviderAdapter::Google(Google::new(settings)?),
        };
        Ok(Self {
            adapter,
            inline_tags: settings.inline_tags,
            prices: settings.prices.clone(),
            rate: settings
                .requests_per_minute
                .map(super::rate::TokenBucket::new),
            account: super::limits::Account::of(settings),
        })
    }

    /// The rate limits the account's last answer reported, to any provider built for it.
    pub fn limits(&self) -> Option<dusk_protocol::model::RateLimits> {
        self.account.limits()
    }
}

impl ModelProvider for Provider {
    fn list_models(&self) -> BoxFuture<'_, Vec<Model>> {
        Box::pin(async move {
            match &self.adapter {
                ProviderAdapter::OpenAiCompatible(adapter) => adapter.list_models().await,
                ProviderAdapter::ChatGpt(adapter) => adapter.list_models().await,
                ProviderAdapter::Anthropic(adapter) => adapter.list_models().await,
                ProviderAdapter::Google(adapter) => adapter.list_models().await,
            }
        })
    }

    fn stream_chat<'a>(&'a self, request: ChatRequest<'a>) -> BoxFuture<'a, Box<dyn ChatStream>> {
        Box::pin(async move {
            if let Some(rate) = &self.rate {
                rate.take().await;
            }
            let price = self.prices.get(request.model).copied();
            let adapter = match &self.adapter {
                ProviderAdapter::OpenAiCompatible(adapter) => {
                    StreamAdapter::OpenAiCompatible(adapter.stream_chat(request).await?)
                }
                ProviderAdapter::ChatGpt(adapter) => {
                    StreamAdapter::ChatGpt(adapter.stream_chat(request).await?)
                }
                ProviderAdapter::Anthropic(adapter) => {
                    StreamAdapter::Anthropic(adapter.stream_chat(request).await?)
                }
                ProviderAdapter::Google(adapter) => {
                    StreamAdapter::Google(adapter.stream_chat(request).await?)
                }
            };
            let stream = ProviderStream {
                adapter,
                tags: self.inline_tags.then(TagSplitter::default),
                pending: VecDeque::new(),
                price,
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
                let mut event = match &mut self.adapter {
                    StreamAdapter::OpenAiCompatible(stream) => stream.next_event().await?,
                    StreamAdapter::ChatGpt(stream) => stream.next_event().await?,
                    StreamAdapter::Anthropic(stream) => stream.next_event().await?,
                    StreamAdapter::Google(stream) => stream.next_event().await?,
                };
                if let (Some(ModelEvent::Usage(usage)), Some(price)) = (&mut event, &self.price)
                    && usage.cost.is_none()
                {
                    usage.cost = super::cost::priced(price, usage);
                }
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
