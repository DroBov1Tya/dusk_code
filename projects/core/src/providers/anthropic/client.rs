//! The Anthropic HTTP client: the model catalogue and the streamed message.

use std::{collections::BTreeMap, time::Duration};

use anyhow::{Result, anyhow, bail, ensure};
use reqwest::{
    Client, RequestBuilder, Response, StatusCode, Url,
    header::{ACCEPT, HeaderName, HeaderValue},
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::{Effort, ProviderConfig},
    providers::{
        errors::{Transient, UnsupportedEffort, UnsupportedPictures},
        registry::Model,
        request::{ChatMessage, ChatRequest},
        transport::http,
    },
};

use super::{request::MessagesRequest, stream::MessageStream};

const API_VERSION: &str = "2023-06-01";
const MAX_ERROR_BYTES: usize = 64 * 1024;
const ERROR_BODY_TIMEOUT: Duration = Duration::from_secs(5);
/// Error messages reach the transcript; a long one is cut.
const MAX_ERROR_CHARS: usize = 300;
const MAX_CATALOGUE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CATALOGUE_PAGES: usize = 10;
const CATALOGUE_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Anthropic {
    client: Client,
    models_url: Url,
    messages_url: Url,
    api_key: Option<HeaderValue>,
}

#[derive(Deserialize)]
struct ModelPage {
    data: Vec<ModelEntry>,
    #[serde(default)]
    has_more: bool,
    last_id: Option<String>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
    max_input_tokens: Option<u64>,
    max_tokens: Option<u64>,
}

impl Anthropic {
    pub fn new(config: &ProviderConfig) -> Result<Self> {
        let base_url = http::base_url(&config.base_url, config.allow_http)?;
        let api_key = if config.api_key.0.is_empty() {
            None
        } else {
            let mut value = HeaderValue::from_str(&config.api_key.0)
                .map_err(|_| anyhow!("invalid provider API key"))?;
            value.set_sensitive(true);
            Some(value)
        };
        let client = http::client()?;
        Ok(Self {
            client,
            models_url: http::endpoint(&base_url, &["models"])?,
            messages_url: http::endpoint(&base_url, &["messages"])?,
            api_key,
        })
    }

    pub async fn list_models(&self) -> Result<Vec<Model>> {
        let mut models = BTreeMap::new();
        let mut after: Option<String> = None;
        for _ in 0..MAX_CATALOGUE_PAGES {
            let mut url = self.models_url.clone();
            url.query_pairs_mut().append_pair("limit", "1000");
            if let Some(after) = &after {
                url.query_pairs_mut().append_pair("after_id", after);
            }
            let page: ModelPage = serde_json::from_slice(&self.catalogue_page(url).await?)
                .map_err(|_| anyhow!("invalid provider model catalogue JSON"))?;
            for model in page.data {
                ensure!(
                    !model.id.trim().is_empty() && !model.id.chars().any(char::is_control),
                    "provider returned an empty or invalid model ID"
                );
                models.insert(model.id, (model.max_input_tokens, model.max_tokens));
            }
            match (page.has_more, page.last_id) {
                (true, Some(last)) => after = Some(last),
                _ => break,
            }
        }
        Ok(models
            .into_iter()
            .map(|(id, (context_window, max_output))| Model {
                id,
                context_window,
                max_output,
            })
            .collect())
    }

    async fn catalogue_page(&self, url: Url) -> Result<Vec<u8>> {
        let mut response = self
            .with_headers(self.client.get(url))
            .header(ACCEPT, "application/json")
            .timeout(CATALOGUE_TIMEOUT)
            .send()
            .await
            .map_err(|_| anyhow!("provider model request failed"))?;
        ensure!(
            response.status().is_success(),
            http::refused("model", response.status())
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
        Ok(bytes)
    }

    pub async fn stream_chat(&self, request: ChatRequest<'_>) -> Result<MessageStream> {
        let effort_off = request.effort == Some(Effort::Off);
        let has_pictures = request
            .messages
            .iter()
            .any(|message| matches!(message, ChatMessage::UserWithPictures { .. }));
        let body = MessagesRequest::new(request);
        let response = self
            .with_headers(self.client.post(self.messages_url.clone()))
            .header(ACCEPT, http::EVENT_STREAM)
            .json(&body)
            .send()
            .await
            .map_err(|_| Transient("provider completion request failed".into()))?;
        let status = response.status();
        if !status.is_success() {
            let reason = tokio::time::timeout(ERROR_BODY_TIMEOUT, error_message(response))
                .await
                .ok()
                .flatten();
            let refused = http::refused("completion", status);
            let message = match &reason {
                Some(reason) => format!("{refused}: {reason}"),
                None => refused,
            };
            if http::is_retryable(status) {
                bail!(Transient(message));
            }
            let reason = reason.unwrap_or_default().to_ascii_lowercase();
            if effort_off && status == StatusCode::BAD_REQUEST && reason.contains("thinking") {
                bail!(UnsupportedEffort);
            }
            if has_pictures && status == StatusCode::BAD_REQUEST && reason.contains("image") {
                bail!(UnsupportedPictures);
            }
            bail!(message);
        }
        ensure!(
            http::is_event_stream(&response),
            "provider did not return a stream"
        );
        Ok(MessageStream::new(response))
    }

    fn with_headers(&self, request: RequestBuilder) -> RequestBuilder {
        let request = request.header("anthropic-version", API_VERSION);
        match &self.api_key {
            Some(key) => request.header(HeaderName::from_static("x-api-key"), key.clone()),
            None => request,
        }
    }
}

/// The API's own words about a failed request, bounded.
async fn error_message(mut response: Response) -> Option<String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if chunk.len() > MAX_ERROR_BYTES - bytes.len() {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: Value = serde_json::from_slice(&bytes).ok()?;
    let message = body.pointer("/error/message")?.as_str()?;
    Some(message.chars().take(MAX_ERROR_CHARS).collect())
}
