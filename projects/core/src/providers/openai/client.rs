//! The OpenAI-compatible HTTP client: the model catalogue and the streamed completion.

use std::{collections::BTreeMap, time::Duration};

use anyhow::{Result, anyhow, bail, ensure};
use reqwest::{
    Client, RequestBuilder, Response, Url,
    header::{ACCEPT, AUTHORIZATION, HeaderValue},
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::{Effort, ProviderConfig, ProviderKind},
    providers::{
        errors::{Transient, UnsupportedEffort, UnsupportedPictures},
        registry::Model,
        request::{ChatMessage, ChatRequest},
        transport::http,
    },
};

use super::{request::CompletionRequest, stream::CompletionStream};

const MAX_ERROR_BYTES: usize = 64 * 1024;
const ERROR_BODY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CATALOGUE_BYTES: usize = 4 * 1024 * 1024;
const CATALOGUE_TIMEOUT: Duration = Duration::from_secs(30);
pub struct OpenAiCompatible {
    client: Client,
    models_url: Url,
    completions_url: Url,
    authorization: Option<HeaderValue>,
    /// OpenAI itself takes `max_completion_tokens`; its reasoning models refuse `max_tokens`.
    completion_tokens: bool,
}

#[derive(Deserialize)]
struct ModelCatalogue {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
    /// Named differently by LiteLLM, OpenRouter and others; OpenAI omits it.
    #[serde(alias = "context_length", alias = "context_window")]
    max_input_tokens: Option<u64>,
}

impl OpenAiCompatible {
    pub fn new(config: &ProviderConfig) -> Result<Self> {
        let base_url = http::base_url(&config.base_url, config.allow_http)?;
        let authorization = if config.api_key.0.is_empty() {
            None
        } else {
            let mut value = HeaderValue::from_str(&format!("Bearer {}", config.api_key.0))
                .map_err(|_| anyhow!("invalid provider API key"))?;
            value.set_sensitive(true);
            Some(value)
        };
        let client = http::client()?;
        Ok(Self {
            client,
            models_url: http::endpoint(&base_url, &["models"])?,
            completions_url: http::endpoint(&base_url, &["chat", "completions"])?,
            authorization,
            completion_tokens: matches!(config.kind, ProviderKind::OpenAi),
        })
    }

    pub async fn list_models(&self) -> Result<Vec<Model>> {
        // Do not retain reqwest errors or response bodies in error chains: either
        // can contain endpoint input or values echoed by an upstream server.
        let mut response = self
            .with_authorization(self.client.get(self.models_url.clone()))
            .header(ACCEPT, "application/json")
            .timeout(CATALOGUE_TIMEOUT)
            .send()
            .await
            .map_err(|_| anyhow!("provider model request failed"))?;
        ensure!(
            response.status().is_success(),
            http::refused("model", response.status())
        );
        ensure!(
            response
                .content_length()
                .is_none_or(|length| length <= MAX_CATALOGUE_BYTES as u64),
            "provider model catalogue is too large"
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow!("cannot read provider model catalogue"))?
        {
            ensure!(
                chunk.len() <= MAX_CATALOGUE_BYTES - bytes.len(),
                "provider model catalogue is too large"
            );
            bytes.extend_from_slice(&chunk);
        }
        let catalogue: ModelCatalogue = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow!("invalid provider model catalogue JSON"))?;
        let mut models = BTreeMap::new();
        for model in catalogue.data {
            ensure!(
                !model.id.trim().is_empty() && !model.id.chars().any(char::is_control),
                "provider returned an empty or invalid model ID"
            );
            models.insert(model.id, model.max_input_tokens);
        }
        Ok(models
            .into_iter()
            .map(|(id, context_window)| Model {
                id,
                context_window,
                max_output: None,
            })
            .collect())
    }

    pub async fn stream_chat(&self, request: ChatRequest<'_>) -> Result<CompletionStream> {
        let effort_off = request.effort == Some(Effort::Off);
        let has_pictures = request
            .messages
            .iter()
            .any(|message| matches!(message, ChatMessage::UserWithPictures { .. }));
        let request = CompletionRequest::new(request, self.completion_tokens);
        let response = self
            .with_authorization(self.client.post(self.completions_url.clone()))
            .header(ACCEPT, http::EVENT_STREAM)
            .json(&request)
            .send()
            .await
            .map_err(|_| Transient("provider completion request failed".into()))?;
        let status = response.status();
        if !status.is_success() {
            let message = http::refused("completion", status);
            if http::is_retryable(status) {
                return Err(http::retry_failure(&response, message));
            }
            if has_pictures && matches!(status.as_u16(), 400 | 413 | 415 | 422) {
                bail!(UnsupportedPictures);
            }
            if effort_off && matches!(status.as_u16(), 400 | 422) {
                // Inspect a bounded JSON error only; echoed provider text never reaches logs.
                if tokio::time::timeout(ERROR_BODY_TIMEOUT, rejects_effort_off(response))
                    .await
                    .unwrap_or(false)
                {
                    bail!(UnsupportedEffort);
                }
            }
            bail!(message);
        }
        ensure!(
            http::is_event_stream(&response),
            "provider did not return a stream"
        );
        Ok(CompletionStream::new(response, effort_off))
    }

    fn with_authorization(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.authorization {
            Some(value) => request.header(AUTHORIZATION, value.clone()),
            None => request,
        }
    }
}

async fn rejects_effort_off(mut response: Response) -> bool {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ERROR_BYTES as u64)
    {
        return false;
    }
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if chunk.len() <= MAX_ERROR_BYTES - bytes.len() => {
                bytes.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            _ => return false,
        }
    }
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|body| body.get("error").cloned())
        .is_some_and(|error| error_rejects_effort_off(&error))
}

/// Only an explicit unsupported parameter/value verdict qualifies. Authentication,
/// generic invalid requests, network failures and server errors never change settings.
pub fn error_rejects_effort_off(error: &Value) -> bool {
    let parameter = error
        .get("param")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let identifies_effort = parameter == "reasoning_effort" || message.contains("reasoning_effort");
    let explicit = matches!(
        code,
        "unsupported_parameter" | "unsupported_value" | "invalid_enum_value"
    ) || [
        "not supported",
        "does not support",
        "unsupported",
        "must be one of",
        "supported values",
    ]
    .iter()
    .any(|phrase| message.contains(phrase));
    identifies_effort && explicit
}
