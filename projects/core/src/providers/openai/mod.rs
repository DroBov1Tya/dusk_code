mod stream;

use std::{collections::BTreeSet, time::Duration};

use anyhow::{Result, anyhow, ensure};
use reqwest::{
    Client, Response, Url,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};

use crate::config::ProviderConfig;

use super::{ChatMessage, ChatRole, Model};

pub(super) use stream::CompletionStream;

const MAX_CATALOGUE_BYTES: usize = 4 * 1024 * 1024;
const CATALOGUE_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest silence tolerated between streamed chunks; reasoning models can pause for a while.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const EVENT_STREAM: &str = "text/event-stream";

pub(super) struct OpenAiCompatible {
    client: Client,
    models_url: Url,
    completions_url: Url,
    authorization: HeaderValue,
}

#[derive(Deserialize)]
struct ModelCatalogue {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

#[derive(Serialize)]
struct CompletionRequest<'a> {
    model: &'a str,
    messages: Vec<WireMessage<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct WireMessage<'a> {
    role: &'static str,
    content: &'a str,
}

impl<'a> From<&'a ChatMessage> for WireMessage<'a> {
    fn from(message: &'a ChatMessage) -> Self {
        let role = match message.role {
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
        };
        Self {
            role,
            content: &message.content,
        }
    }
}

impl OpenAiCompatible {
    pub(super) fn new(config: &ProviderConfig) -> Result<Self> {
        let base_url =
            Url::parse(&config.base_url).map_err(|_| anyhow!("invalid provider base URL"))?;
        ensure!(
            base_url.scheme() == "https" && base_url.host_str().is_some(),
            "provider base URL must use HTTPS"
        );
        ensure!(
            base_url.username().is_empty()
                && base_url.password().is_none()
                && base_url.query().is_none()
                && base_url.fragment().is_none(),
            "provider base URL must not contain credentials, a query or a fragment"
        );
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", config.api_key.0))
            .map_err(|_| anyhow!("invalid provider API key"))?;
        authorization.set_sensitive(true);
        // No total timeout: it would cut long streams. Requests bound their own duration.
        let client = Client::builder()
            .redirect(Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(STREAM_IDLE_TIMEOUT)
            .build()
            .map_err(|_| anyhow!("cannot build provider HTTP client"))?;
        Ok(Self {
            client,
            models_url: endpoint(&base_url, &["models"])?,
            completions_url: endpoint(&base_url, &["chat", "completions"])?,
            authorization,
        })
    }

    pub(super) async fn list_models(&self) -> Result<Vec<Model>> {
        // Do not retain reqwest errors or response bodies in error chains: either
        // can contain endpoint input or values echoed by an upstream server.
        let mut response = self
            .client
            .get(self.models_url.clone())
            .header(AUTHORIZATION, self.authorization.clone())
            .header(ACCEPT, "application/json")
            .timeout(CATALOGUE_TIMEOUT)
            .send()
            .await
            .map_err(|_| anyhow!("provider model request failed"))?;
        ensure!(
            response.status().is_success(),
            "provider model request returned HTTP {}",
            response.status().as_u16()
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
        let mut ids = BTreeSet::new();
        for model in catalogue.data {
            ensure!(
                !model.id.trim().is_empty() && !model.id.chars().any(char::is_control),
                "provider returned an empty or invalid model ID"
            );
            ids.insert(model.id);
        }
        Ok(ids.into_iter().map(|id| Model { id }).collect())
    }

    pub(super) async fn stream_chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
    ) -> Result<CompletionStream> {
        let request = CompletionRequest {
            model,
            messages: messages.iter().map(WireMessage::from).collect(),
            stream: true,
        };
        let response = self
            .client
            .post(self.completions_url.clone())
            .header(AUTHORIZATION, self.authorization.clone())
            .header(ACCEPT, EVENT_STREAM)
            .json(&request)
            .send()
            .await
            .map_err(|_| anyhow!("provider completion request failed"))?;
        ensure!(
            response.status().is_success(),
            "provider completion request returned HTTP {}",
            response.status().as_u16()
        );
        ensure!(
            is_event_stream(&response),
            "provider did not return a stream"
        );
        Ok(CompletionStream::new(response))
    }
}

fn endpoint(base_url: &Url, segments: &[&str]) -> Result<Url> {
    let mut url = base_url.clone();
    url.path_segments_mut()
        .map_err(|_| anyhow!("invalid provider base URL path"))?
        .pop_if_empty()
        .extend(segments);
    Ok(url)
}

fn is_event_stream(response: &Response) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with(EVENT_STREAM))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_extend_base_path() {
        let base = Url::parse("https://host/v1/").unwrap();
        assert_eq!(
            endpoint(&base, &["chat", "completions"]).unwrap().as_str(),
            "https://host/v1/chat/completions"
        );
        let base = Url::parse("https://host/v1").unwrap();
        assert_eq!(
            endpoint(&base, &["models"]).unwrap().as_str(),
            "https://host/v1/models"
        );
    }

    #[test]
    fn request_carries_only_role_and_text() {
        let messages = [ChatMessage {
            role: ChatRole::User,
            content: "hi".into(),
        }];
        let request = CompletionRequest {
            model: "m",
            messages: messages.iter().map(WireMessage::from).collect(),
            stream: true,
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({
                "model": "m",
                "messages": [{"role": "user", "content": "hi"}],
                "stream": true
            })
        );
    }
}
