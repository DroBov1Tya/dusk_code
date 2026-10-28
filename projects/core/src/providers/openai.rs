//! OpenAI-compatible Chat Completions API (OpenAI, LiteLLM, vLLM and others):
//! the HTTP client, then the streamed response it returns.

use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};

use anyhow::{Result, anyhow, bail, ensure};
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    chat::{ChatMessage, ChatRequest, ToolSpec},
    model_output::{FinishReason, ModelEvent, ToolCall, Usage},
    provider::{Model, Transient},
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
    /// Named differently by LiteLLM, OpenRouter and others; OpenAI omits it.
    #[serde(alias = "context_length", alias = "context_window")]
    max_input_tokens: Option<u64>,
}

/// The request body. Owned copies keep these types free of borrowed lifetimes; the
/// copying costs nothing next to the network call.
#[derive(Serialize)]
struct CompletionRequest {
    model: String,
    messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTool>,
    stream: bool,
    stream_options: StreamOptions,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
enum WireMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        content: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<WireToolCall>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Serialize)]
struct WireToolCall {
    id: String,
    r#type: &'static str,
    function: WireFunctionCall,
}

#[derive(Serialize)]
struct WireFunctionCall {
    name: String,
    /// The API carries arguments as a JSON string, not an object.
    arguments: String,
}

#[derive(Serialize)]
struct WireTool {
    r#type: &'static str,
    function: WireFunction,
}

#[derive(Serialize)]
struct WireFunction {
    name: String,
    description: String,
    parameters: Value,
}

impl CompletionRequest {
    fn new(request: ChatRequest<'_>) -> Self {
        Self {
            model: request.model.to_owned(),
            messages: request.messages.iter().map(WireMessage::from).collect(),
            tools: request.tools.iter().map(WireTool::from).collect(),
            stream: true,
            stream_options: StreamOptions {
                include_usage: true,
            },
        }
    }
}

impl From<&ChatMessage> for WireMessage {
    fn from(message: &ChatMessage) -> Self {
        match message.clone() {
            ChatMessage::System(content) => Self::System { content },
            ChatMessage::User(content) => Self::User { content },
            ChatMessage::Assistant { text, tool_calls } => Self::Assistant {
                content: text,
                tool_calls: tool_calls.iter().map(WireToolCall::from).collect(),
            },
            ChatMessage::Tool { call_id, content } => Self::Tool {
                tool_call_id: call_id,
                content,
            },
        }
    }
}

impl From<&ToolCall> for WireToolCall {
    fn from(call: &ToolCall) -> Self {
        Self {
            id: call.id.clone(),
            r#type: "function",
            function: WireFunctionCall {
                name: call.name.clone(),
                arguments: call.arguments.to_string(),
            },
        }
    }
}

impl From<&ToolSpec> for WireTool {
    fn from(tool: &ToolSpec) -> Self {
        Self {
            r#type: "function",
            function: WireFunction {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            },
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
            .map(|(id, context_window)| Model { id, context_window })
            .collect())
    }

    pub(super) async fn stream_chat(&self, request: ChatRequest<'_>) -> Result<CompletionStream> {
        let request = CompletionRequest::new(request);
        let response = self
            .client
            .post(self.completions_url.clone())
            .header(AUTHORIZATION, self.authorization.clone())
            .header(ACCEPT, EVENT_STREAM)
            .json(&request)
            .send()
            .await
            .map_err(|_| Transient("provider completion request failed".into()))?;
        let status = response.status();
        if !status.is_success() {
            let message = format!(
                "provider completion request returned HTTP {}",
                status.as_u16()
            );
            if is_retryable(status) {
                bail!(Transient(message));
            }
            bail!(message);
        }
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

/// Timeouts, conflicts, rate limits and server errors may pass on a later attempt.
fn is_retryable(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 409 | 429) || status.is_server_error()
}

fn is_event_stream(response: &Response) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with(EVENT_STREAM))
}

/// Events of one streamed response, read lazily from the network.
pub(super) struct CompletionStream {
    response: Response,
    parser: CompletionParser,
    pending: VecDeque<ModelEvent>,
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

    pub(super) async fn next_event(&mut self) -> Result<Option<ModelEvent>> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
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
                .map_err(|_| Transient("cannot read provider stream".into()))?;
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
    usage: Option<WireUsage>,
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
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
}

/// The first delta of a call carries its ID and name; later ones only argument pieces.
#[derive(Deserialize)]
struct ToolCallDelta {
    index: u32,
    id: Option<String>,
    function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

impl CompletionParser {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<ModelEvent>> {
        let mut events = Vec::new();
        for data in self.decoder.feed(bytes)? {
            if self.done {
                break;
            }
            if data == DONE_MARKER {
                self.finish()?;
                break;
            }
            events.extend(self.parse_chunk(&data)?);
        }
        Ok(events)
    }

    /// The model is done once it reported a finish reason; `[DONE]` or EOF alone is a cut stream.
    fn finish(&mut self) -> Result<()> {
        self.done = true;
        if !self.finish_seen {
            bail!(Transient(
                "provider stream ended before the response was complete".into()
            ));
        }
        Ok(())
    }

    fn parse_chunk(&mut self, data: &str) -> Result<Vec<ModelEvent>> {
        // Upstream payloads can echo input; never copy them into errors.
        let chunk: Chunk = serde_json::from_str(data)
            .map_err(|_| anyhow!("invalid provider stream chunk JSON"))?;
        if chunk.error.is_some() {
            bail!("provider reported an error during streaming");
        }
        let mut events = Vec::new();
        for choice in chunk.choices.into_iter().filter(|choice| choice.index == 0) {
            if let Some(delta) = choice.delta {
                events.extend(delta_events(delta));
            }
            if let Some(reason) = choice.finish_reason {
                events.push(ModelEvent::Finished(finish_reason(&reason)?));
                self.finish_seen = true;
            }
        }
        if let Some(usage) = chunk.usage {
            events.push(ModelEvent::Usage(Usage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
            }));
        }
        Ok(events)
    }
}

fn delta_events(delta: Delta) -> Vec<ModelEvent> {
    let mut events = Vec::new();
    let non_empty = |text: Option<String>| text.filter(|text| !text.is_empty());
    events.extend(non_empty(delta.reasoning_content).map(ModelEvent::Reasoning));
    events.extend(non_empty(delta.content).map(ModelEvent::Text));
    for call in delta.tool_calls {
        let (name, arguments) = match call.function {
            Some(function) => (function.name, function.arguments),
            None => (None, None),
        };
        if let Some(name) = name {
            let id = call.id.unwrap_or_else(|| format!("call_{}", call.index));
            events.push(ModelEvent::ToolCall {
                index: call.index,
                id,
                name,
            });
        }
        if let Some(fragment) = non_empty(arguments) {
            events.push(ModelEvent::ToolArguments {
                index: call.index,
                fragment,
            });
        }
    }
    events
}

fn finish_reason(reason: &str) -> Result<FinishReason> {
    Ok(match reason {
        "stop" => FinishReason::Stop,
        "tool_calls" | "function_call" => FinishReason::ToolCalls,
        "length" => FinishReason::Length,
        "content_filter" => FinishReason::ContentFilter,
        _ => bail!("model stopped for an unsupported reason"),
    })
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
    fn request_carries_roles_tools_and_call_ids() {
        let call = ToolCall {
            id: "c1".into(),
            name: "weather".into(),
            arguments: serde_json::json!({"city": "Kazan"}),
        };
        let messages = [
            ChatMessage::User("hi".into()),
            ChatMessage::Assistant {
                text: String::new(),
                tool_calls: vec![call],
            },
            ChatMessage::Tool {
                call_id: "c1".into(),
                content: "sunny".into(),
            },
        ];
        let tools = [ToolSpec {
            name: "weather".into(),
            description: "Current weather".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let request = CompletionRequest::new(ChatRequest {
            model: "m",
            messages: &messages,
            tools: &tools,
        });
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            serde_json::json!({
                "model": "m",
                "messages": [
                    {"role": "user", "content": "hi"},
                    {"role": "assistant", "content": "", "tool_calls": [{
                        "id": "c1", "type": "function",
                        "function": {"name": "weather", "arguments": "{\"city\":\"Kazan\"}"}
                    }]},
                    {"role": "tool", "tool_call_id": "c1", "content": "sunny"}
                ],
                "tools": [{"type": "function", "function": {
                    "name": "weather", "description": "Current weather",
                    "parameters": {"type": "object"}
                }}],
                "stream": true,
                "stream_options": {"include_usage": true}
            })
        );
    }

    fn chunk(delta: &str) -> String {
        format!("data: {{\"choices\":[{{\"index\":0,\"delta\":{delta}}}]}}\n\n")
    }

    fn finish(reason: &str) -> String {
        format!(
            "data: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"{reason}\"}}]}}\n\n"
        )
    }

    #[test]
    fn emits_reasoning_text_finish_and_usage_in_order() {
        let body = [
            chunk(r#"{"role":"assistant","content":""}"#),
            chunk(r#"{"reasoning_content":"think"}"#),
            chunk(r#"{"content":"Hi"}"#),
            finish("stop"),
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n"
                .to_owned(),
            "data: [DONE]\n\n".to_owned(),
        ]
        .concat();
        let mut parser = CompletionParser::default();
        let events = parser.feed(body.as_bytes()).unwrap();
        assert_eq!(
            events,
            [
                ModelEvent::Reasoning("think".into()),
                ModelEvent::Text("Hi".into()),
                ModelEvent::Finished(FinishReason::Stop),
                ModelEvent::Usage(Usage {
                    prompt_tokens: 5,
                    completion_tokens: 2
                }),
            ]
        );
        assert!(parser.done);
    }

    /// Recorded from a LiteLLM server: one call opens with ID and name, then argument pieces.
    #[test]
    fn emits_streamed_tool_calls() {
        let body = [
            chunk(r#"{"tool_calls":[{"id":"call_a","function":{"arguments":"","name":"weather"},"type":"function","index":0}]}"#),
            chunk(r#"{"tool_calls":[{"function":{"arguments":"{\"city\":"},"type":"function","index":0}]}"#),
            chunk(r#"{"tool_calls":[{"function":{"arguments":"\"Kazan\"}"},"type":"function","index":0}]}"#),
            finish("tool_calls"),
        ]
        .concat();
        let events = CompletionParser::default().feed(body.as_bytes()).unwrap();
        assert_eq!(
            events,
            [
                ModelEvent::ToolCall {
                    index: 0,
                    id: "call_a".into(),
                    name: "weather".into()
                },
                ModelEvent::ToolArguments {
                    index: 0,
                    fragment: "{\"city\":".into()
                },
                ModelEvent::ToolArguments {
                    index: 0,
                    fragment: "\"Kazan\"}".into()
                },
                ModelEvent::Finished(FinishReason::ToolCalls),
            ]
        );
    }

    #[test]
    fn accepts_reasoning_field_alias() {
        let events = CompletionParser::default()
            .feed(chunk(r#"{"reasoning":"r"}"#).as_bytes())
            .unwrap();
        assert_eq!(events, [ModelEvent::Reasoning("r".into())]);
    }

    #[test]
    fn eof_after_finish_reason_is_complete() {
        let mut parser = CompletionParser::default();
        parser.feed(finish("length").as_bytes()).unwrap();
        assert!(parser.finish().is_ok());
    }

    #[test]
    fn only_passing_failures_are_transient() {
        assert!(is_retryable(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable(StatusCode::BAD_GATEWAY));
        assert!(!is_retryable(StatusCode::UNAUTHORIZED));
        assert!(!is_retryable(StatusCode::BAD_REQUEST));
        let cut = CompletionParser::default().finish().unwrap_err();
        assert!(crate::providers::is_transient(&cut));
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
    fn unknown_finish_reason_is_an_error() {
        assert!(
            CompletionParser::default()
                .feed(finish("mystery").as_bytes())
                .is_err()
        );
    }

    #[test]
    fn ignores_data_after_done() {
        let body = format!(
            "{}data: [DONE]\n\n{}",
            finish("stop"),
            chunk(r#"{"content":"late"}"#)
        );
        let events = CompletionParser::default().feed(body.as_bytes()).unwrap();
        assert_eq!(events, [ModelEvent::Finished(FinishReason::Stop)]);
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
