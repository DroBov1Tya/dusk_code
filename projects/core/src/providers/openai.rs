//! OpenAI-compatible Chat Completions API (OpenAI, LiteLLM, vLLM and others):
//! the HTTP client, then the streamed response it returns.

use std::{
    collections::{BTreeSet, VecDeque},
    time::Duration,
};

use anyhow::{Result, anyhow, bail, ensure};
use reqwest::{
    Client, Response, Url,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};

use super::{
    chat::{ChatDelta, ChatMessage, ChatRole},
    provider::Model,
    sse::SseDecoder,
};
use crate::config::ProviderConfig;

const MAX_CATALOGUE_BYTES: usize = 4 * 1024 * 1024;
const CATALOGUE_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest silence tolerated between streamed chunks; reasoning models can pause for a while.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const EVENT_STREAM: &str = "text/event-stream";
const DONE_MARKER: &str = "[DONE]";

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
            ChatRole::System => "system",
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

/// Deltas of one streamed response, read lazily from the network.
pub(super) struct CompletionStream {
    response: Response,
    parser: CompletionParser,
    pending: VecDeque<ChatDelta>,
    finished: bool,
}

impl CompletionStream {
    fn new(response: Response) -> Self {
        Self {
            response,
            parser: CompletionParser::default(),
            pending: VecDeque::new(),
            finished: false,
        }
    }

    pub(super) async fn next_delta(&mut self) -> Result<Option<ChatDelta>> {
        loop {
            if let Some(delta) = self.pending.pop_front() {
                return Ok(Some(delta));
            }
            if self.finished {
                return Ok(None);
            }
            // Mark first so an error also ends the stream.
            self.finished = true;
            let chunk = self
                .response
                .chunk()
                .await
                .map_err(|_| anyhow!("cannot read provider stream"))?;
            match chunk {
                Some(bytes) => {
                    self.pending.extend(self.parser.feed(&bytes)?);
                    self.finished = self.parser.done;
                }
                None => self.parser.finish()?,
            }
        }
    }
}

/// Network-independent parser, so the protocol can be tested from raw bytes.
#[derive(Default)]
struct CompletionParser {
    decoder: SseDecoder,
    finish_seen: bool,
    done: bool,
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<Choice>,
    error: Option<serde::de::IgnoredAny>,
}

#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    index: u32,
    delta: Option<Delta>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
    /// Field name differs between servers (vLLM/LiteLLM vs. OpenRouter).
    #[serde(alias = "reasoning")]
    reasoning_content: Option<String>,
}

impl CompletionParser {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<ChatDelta>> {
        let mut deltas = Vec::new();
        for data in self.decoder.feed(bytes)? {
            if self.done {
                break;
            }
            if data == DONE_MARKER {
                self.finish()?;
                break;
            }
            deltas.extend(self.parse_chunk(&data)?);
        }
        Ok(deltas)
    }

    /// The model is done once it reported a finish reason; `[DONE]` or EOF alone is a cut stream.
    fn finish(&mut self) -> Result<()> {
        self.done = true;
        ensure!(
            self.finish_seen,
            "provider stream ended before the response was complete"
        );
        Ok(())
    }

    fn parse_chunk(&mut self, data: &str) -> Result<Vec<ChatDelta>> {
        // Upstream payloads can echo input; never copy them into errors.
        let chunk: Chunk = serde_json::from_str(data)
            .map_err(|_| anyhow!("invalid provider stream chunk JSON"))?;
        if chunk.error.is_some() {
            bail!("provider reported an error during streaming");
        }
        let mut deltas = Vec::new();
        for choice in chunk.choices.into_iter().filter(|choice| choice.index == 0) {
            if let Some(delta) = choice.delta {
                deltas.extend(
                    delta
                        .reasoning_content
                        .filter(|text| !text.is_empty())
                        .map(ChatDelta::Reasoning),
                );
                deltas.extend(
                    delta
                        .content
                        .filter(|text| !text.is_empty())
                        .map(ChatDelta::Text),
                );
            }
            if let Some(reason) = choice.finish_reason {
                check_finish_reason(&reason)?;
                self.finish_seen = true;
            }
        }
        Ok(deltas)
    }
}

/// Only a natural stop is a complete answer; a cut one must not be stored as completed.
fn check_finish_reason(reason: &str) -> Result<()> {
    match reason {
        "stop" => Ok(()),
        "length" => bail!("model answer was cut by the token limit"),
        "content_filter" => bail!("model answer was blocked by the content filter"),
        _ => bail!("model stopped for an unsupported reason"),
    }
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

    fn chunk(delta: &str) -> String {
        format!("data: {{\"choices\":[{{\"index\":0,\"delta\":{delta}}}]}}\n\n")
    }

    const FINISH: &str =
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";

    #[test]
    fn splits_reasoning_and_text_in_order() {
        let body = [
            chunk(r#"{"role":"assistant","content":""}"#),
            chunk(r#"{"reasoning_content":"think"}"#),
            chunk(r#"{"content":"Hi"}"#),
            FINISH.to_owned(),
            "data: {\"choices\":[],\"usage\":{\"total_tokens\":3}}\n\n".to_owned(),
            "data: [DONE]\n\n".to_owned(),
        ]
        .concat();
        let mut parser = CompletionParser::default();
        let deltas = parser.feed(body.as_bytes()).unwrap();
        assert_eq!(
            deltas,
            [
                ChatDelta::Reasoning("think".into()),
                ChatDelta::Text("Hi".into())
            ]
        );
        assert!(parser.done);
    }

    #[test]
    fn accepts_reasoning_field_alias() {
        let mut parser = CompletionParser::default();
        let deltas = parser
            .feed(chunk(r#"{"reasoning":"r"}"#).as_bytes())
            .unwrap();
        assert_eq!(deltas, [ChatDelta::Reasoning("r".into())]);
    }

    #[test]
    fn eof_after_finish_reason_is_complete() {
        let mut parser = CompletionParser::default();
        parser.feed(FINISH.as_bytes()).unwrap();
        assert!(parser.finish().is_ok());
    }

    #[test]
    fn cut_stream_is_an_error() {
        let mut parser = CompletionParser::default();
        parser
            .feed(chunk(r#"{"content":"partial"}"#).as_bytes())
            .unwrap();
        assert!(parser.finish().is_err());
        assert!(
            CompletionParser::default()
                .feed(b"data: [DONE]\n\n")
                .is_err()
        );
    }

    #[test]
    fn truncated_answer_is_an_error() {
        let body =
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"length\"}]}\n\n";
        assert!(CompletionParser::default().feed(body.as_bytes()).is_err());
    }

    #[test]
    fn ignores_data_after_done() {
        let body = format!("{FINISH}data: [DONE]\n\n{}", chunk(r#"{"content":"late"}"#));
        let mut parser = CompletionParser::default();
        assert!(parser.feed(body.as_bytes()).unwrap().is_empty());
    }

    #[test]
    fn stream_error_does_not_echo_upstream_message() {
        let body = "data: {\"error\":{\"message\":\"secret echo\"}}\n\n";
        let error = CompletionParser::default()
            .feed(body.as_bytes())
            .unwrap_err();
        assert!(!format!("{error:#}").contains("secret"));
    }
}
