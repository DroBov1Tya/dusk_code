//! The Gemini API HTTP client: the model catalogue and the streamed answer.

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

use anyhow::{Result, anyhow, bail, ensure};
use parking_lot::RwLock;
use reqwest::{
    Client, RequestBuilder, Response, StatusCode, Url,
    header::{ACCEPT, HeaderName, HeaderValue},
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::ProviderConfig,
    providers::{
        errors::{Transient, UnsupportedPictures},
        registry::Model,
        request::{ChatMessage, ChatRequest},
        transport::http,
    },
};

use super::{request::GenerateRequest, signatures::Signatures, stream::GenerateStream};

const MAX_ERROR_BYTES: usize = 64 * 1024;
const ERROR_BODY_TIMEOUT: Duration = Duration::from_secs(5);
/// Error messages reach the transcript; a long one is cut.
const MAX_ERROR_CHARS: usize = 300;
const MAX_CATALOGUE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CATALOGUE_PAGES: usize = 10;
const CATALOGUE_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Google {
    client: Client,
    base_url: Url,
    /// Sent in a header, never in the URL, where logs and proxies would keep it.
    api_key: Option<HeaderValue>,
    signatures: Arc<Signatures>,
    /// Models whose catalogue entry says they cannot think; they refuse a thinking config.
    plain: RwLock<HashSet<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelPage {
    #[serde(default)]
    models: Vec<ModelEntry>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelEntry {
    /// `models/{id}`.
    name: String,
    input_token_limit: Option<u64>,
    output_token_limit: Option<u64>,
    supported_generation_methods: Option<Vec<String>>,
    thinking: Option<bool>,
}

impl Google {
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
        // Fails here, not at the first request, when the base URL cannot take a path.
        http::endpoint(&base_url, &["models"])?;
        Ok(Self {
            client: http::client()?,
            base_url,
            api_key,
            signatures: Arc::default(),
            plain: RwLock::default(),
        })
    }

    /// The models that generate content; embedding and other models are left out.
    pub async fn list_models(&self) -> Result<Vec<Model>> {
        let mut models = BTreeMap::new();
        let mut plain = HashSet::new();
        let mut page_token: Option<String> = None;
        for _ in 0..MAX_CATALOGUE_PAGES {
            let mut url = http::endpoint(&self.base_url, &["models"])?;
            url.query_pairs_mut().append_pair("pageSize", "1000");
            if let Some(token) = &page_token {
                url.query_pairs_mut().append_pair("pageToken", token);
            }
            let page: ModelPage = serde_json::from_slice(&self.catalogue_page(url).await?)
                .map_err(|_| anyhow!("invalid provider model catalogue JSON"))?;
            for model in page.models {
                let id = model.name.strip_prefix("models/").unwrap_or(&model.name);
                ensure!(
                    !id.trim().is_empty() && !id.chars().any(char::is_control),
                    "provider returned an empty or invalid model ID"
                );
                let generates = model
                    .supported_generation_methods
                    .is_none_or(|methods| methods.iter().any(|method| method == "generateContent"));
                if !generates {
                    continue;
                }
                if model.thinking == Some(false) {
                    plain.insert(id.to_owned());
                }
                models.insert(
                    id.to_owned(),
                    (model.input_token_limit, model.output_token_limit),
                );
            }
            match page.next_page_token.filter(|token| !token.is_empty()) {
                Some(token) => page_token = Some(token),
                None => break,
            }
        }
        *self.plain.write() = plain;
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
            .with_key(self.client.get(url))
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

    pub async fn stream_chat(&self, request: ChatRequest<'_>) -> Result<GenerateStream> {
        let has_pictures = request
            .messages
            .iter()
            .any(|message| matches!(message, ChatMessage::UserWithPictures { .. }));
        // One path segment, so a model name cannot reach another endpoint.
        let method = format!("{}:streamGenerateContent", request.model);
        let mut url = http::endpoint(&self.base_url, &["models", &method])?;
        url.query_pairs_mut().append_pair("alt", "sse");
        let thinks = !self.plain.read().contains(request.model);
        let body = GenerateRequest::new(request, &self.signatures, thinks);
        let response = self
            .with_key(self.client.post(url))
            .header(ACCEPT, http::EVENT_STREAM)
            .json(&body)
            .send()
            .await
            .map_err(|_| Transient("provider completion request failed".into()))?;
        let status = response.status();
        if !status.is_success() {
            let wait = http::retry_failure(&response, String::new());
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
                // Keeps the retry-after the headers gave, with this message.
                return Err(match crate::providers::retry_after(&wait) {
                    Some(after) => anyhow::Error::new(Transient(message))
                        .context(crate::providers::RetryAfter(after)),
                    None => Transient(message).into(),
                });
            }
            let reason = reason.unwrap_or_default().to_ascii_lowercase();
            if has_pictures && status == StatusCode::BAD_REQUEST && reason.contains("image") {
                bail!(UnsupportedPictures);
            }
            bail!(message);
        }
        ensure!(
            http::is_event_stream(&response),
            "provider did not return a stream"
        );
        Ok(GenerateStream::new(response, Arc::clone(&self.signatures)))
    }

    fn with_key(&self, request: RequestBuilder) -> RequestBuilder {
        match &self.api_key {
            Some(key) => request.header(HeaderName::from_static("x-goog-api-key"), key.clone()),
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
