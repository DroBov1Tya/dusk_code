//! A configured provider, dispatching to its protocol adapter.

use anyhow::{Context, Result};

use super::{
    chat::{ChatDelta, ChatMessage, ChatStream},
    completion_stream::CompletionStream,
    model::{Model, ModelProvider},
    openai_compatible::OpenAiCompatible,
};

/// A configured model provider.
pub struct Provider {
    adapter: ProviderAdapter,
}

pub(super) enum ProviderAdapter {
    OpenAiCompatible(OpenAiCompatible),
}

/// A streamed answer from any provider.
pub struct ProviderStream {
    adapter: StreamAdapter,
}

enum StreamAdapter {
    OpenAiCompatible(CompletionStream),
}

impl Provider {
    pub(super) fn new(adapter: ProviderAdapter) -> Self {
        Self { adapter }
    }

    /// Picks the first model of the catalogue so callers need no configured model ID.
    pub async fn default_model(&self) -> Result<Model> {
        self.list_models()
            .await?
            .into_iter()
            .next()
            .context("provider offers no models")
    }

    /// Sends the conversation and returns the answer as a stream.
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
