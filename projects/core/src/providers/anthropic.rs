//! Anthropic Messages API: the HTTP client, then the streamed response it returns.
//! Thinking blocks are not sent back: the history keeps no signatures, and a history
//! without thinking blocks is one the API accepts on every model.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    time::Duration,
};

use anyhow::{Result, anyhow, bail, ensure};
use reqwest::{
    Client, RequestBuilder, Response, StatusCode, Url,
    header::{ACCEPT, CONTENT_TYPE, HeaderName, HeaderValue},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    chat::{ChatMessage, ChatRequest, ToolSpec},
    model_output::{FinishReason, ModelEvent, ToolCall, Usage},
    openai::is_loopback,
    provider::{Model, Transient, UnsupportedEffort, UnsupportedPictures},
    sse::SseDecoder,
};
use crate::config::{Effort, ProviderConfig};

const API_VERSION: &str = "2023-06-01";
const MAX_ERROR_BYTES: usize = 64 * 1024;
const ERROR_BODY_TIMEOUT: Duration = Duration::from_secs(5);
/// Error messages reach the transcript; a long one is cut.
const MAX_ERROR_CHARS: usize = 300;
const MAX_CATALOGUE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CATALOGUE_PAGES: usize = 10;
const CATALOGUE_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest silence tolerated between streamed events; the API pings while it thinks.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const EVENT_STREAM: &str = "text/event-stream";

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
}

#[derive(Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    system: String,
    messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTool>,
    thinking: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<Value>,
    /// Caches the longest prefix the API can; every request repeats the last one.
    cache_control: Value,
    stream: bool,
}

#[derive(Serialize)]
struct WireMessage {
    role: &'static str,
    content: Vec<Value>,
}

#[derive(Serialize)]
struct WireTool {
    name: String,
    description: String,
    input_schema: Value,
}

impl MessagesRequest {
    fn new(request: ChatRequest<'_>) -> Self {
        let (system, messages) = wire_messages(request.messages);
        // Off is the only level without thinking; the rest set how much of it.
        let (thinking, effort) = match request.effort {
            Some(Effort::Off) => (json!({"type": "disabled"}), None),
            effort => (
                json!({"type": "adaptive", "display": "summarized"}),
                effort.map(wire_effort),
            ),
        };
        Self {
            model: request.model.to_owned(),
            max_tokens: request.max_output_tokens,
            system,
            messages,
            tools: request.tools.iter().map(WireTool::from).collect(),
            thinking,
            output_config: effort.map(|effort| json!({ "effort": effort })),
            cache_control: json!({"type": "ephemeral"}),
            stream: true,
        }
    }
}

fn wire_effort(effort: Effort) -> &'static str {
    match effort {
        Effort::Off | Effort::Minimal | Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::Xhigh => "xhigh",
        Effort::Max | Effort::Ultra => "max",
    }
}

/// The leading system messages become the system prompt; a later one joins the user's
/// turn as text. Turns of one role in a row are one message, so the tool results of a
/// step arrive together, as the API wants them.
fn wire_messages(messages: &[ChatMessage]) -> (String, Vec<WireMessage>) {
    let mut system = Vec::new();
    let mut wire: Vec<WireMessage> = Vec::new();
    for message in messages {
        let (role, blocks) = match message {
            ChatMessage::System(text) if wire.is_empty() => {
                system.push(text.as_str());
                continue;
            }
            ChatMessage::System(text) => ("user", text_block(&format!("[system] {text}"))),
            ChatMessage::User(text) => ("user", text_block(text)),
            ChatMessage::UserWithPictures { text, pictures } => {
                let mut blocks = text_block(text);
                blocks.extend(pictures.iter().map(|picture| {
                    json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": picture.mime_type,
                            "data": picture.base64,
                        },
                    })
                }));
                ("user", blocks)
            }
            ChatMessage::Assistant { text, tool_calls } => {
                let mut blocks = text_block(text);
                blocks.extend(tool_calls.iter().map(tool_use_block));
                ("assistant", blocks)
            }
            ChatMessage::Tool { call_id, content } => (
                "user",
                vec![json!({
                    "type": "tool_result",
                    "tool_use_id": call_id,
                    "content": content,
                })],
            ),
        };
        if blocks.is_empty() {
            continue;
        }
        match wire.last_mut() {
            Some(last) if last.role == role => last.content.extend(blocks),
            _ => wire.push(WireMessage {
                role,
                content: blocks,
            }),
        }
    }
    (system.join("\n\n"), wire)
}

/// The API refuses empty text blocks.
fn text_block(text: &str) -> Vec<Value> {
    match text.trim().is_empty() {
        true => Vec::new(),
        false => vec![json!({"type": "text", "text": text})],
    }
}

fn tool_use_block(call: &ToolCall) -> Value {
    // Input must be an object; arguments that were not JSON go back as none.
    let input = match &call.arguments {
        Value::Object(_) => call.arguments.clone(),
        _ => json!({}),
    };
    json!({"type": "tool_use", "id": call.id, "name": call.name, "input": input})
}

impl From<&ToolSpec> for WireTool {
    fn from(tool: &ToolSpec) -> Self {
        Self {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.parameters.clone(),
        }
    }
}

impl Anthropic {
    pub fn new(config: &ProviderConfig) -> Result<Self> {
        let base_url =
            Url::parse(&config.base_url).map_err(|_| anyhow!("invalid provider base URL"))?;
        ensure!(
            base_url.scheme() == "https" && base_url.host_str().is_some() || is_loopback(&base_url),
            "provider base URL must use HTTPS, or HTTP to 127.0.0.1 or [::1]"
        );
        ensure!(
            base_url.username().is_empty()
                && base_url.password().is_none()
                && base_url.query().is_none()
                && base_url.fragment().is_none(),
            "provider base URL must not contain credentials, a query or a fragment"
        );
        let api_key = if config.api_key.0.is_empty() {
            None
        } else {
            let mut value = HeaderValue::from_str(&config.api_key.0)
                .map_err(|_| anyhow!("invalid provider API key"))?;
            value.set_sensitive(true);
            Some(value)
        };
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
            messages_url: endpoint(&base_url, &["messages"])?,
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
                models.insert(model.id, model.max_input_tokens);
            }
            match (page.has_more, page.last_id) {
                (true, Some(last)) => after = Some(last),
                _ => break,
            }
        }
        Ok(models
            .into_iter()
            .map(|(id, context_window)| Model { id, context_window })
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
            "provider model request returned HTTP {}",
            response.status().as_u16()
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
            .header(ACCEPT, EVENT_STREAM)
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
            let message = match &reason {
                Some(reason) => format!(
                    "provider completion request returned HTTP {}: {reason}",
                    status.as_u16()
                ),
                None => format!(
                    "provider completion request returned HTTP {}",
                    status.as_u16()
                ),
            };
            if is_retryable(status) {
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
        let is_stream = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with(EVENT_STREAM));
        ensure!(is_stream, "provider did not return a stream");
        Ok(MessageStream {
            response,
            parser: MessageParser::default(),
            pending: VecDeque::new(),
            finished: false,
        })
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

fn endpoint(base_url: &Url, segments: &[&str]) -> Result<Url> {
    let mut url = base_url.clone();
    url.path_segments_mut()
        .map_err(|_| anyhow!("invalid provider base URL path"))?
        .pop_if_empty()
        .extend(segments);
    Ok(url)
}

/// Timeouts, conflicts, rate limits, overload (529) and server errors may pass later.
fn is_retryable(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 409 | 429) || status.is_server_error()
}

/// Events of one streamed response, read lazily from the network.
pub struct MessageStream {
    response: Response,
    parser: MessageParser,
    pending: VecDeque<ModelEvent>,
    finished: bool,
}

impl MessageStream {
    pub async fn next_event(&mut self) -> Result<Option<ModelEvent>> {
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
struct MessageParser {
    decoder: SseDecoder,
    /// Content block index to tool call index: calls are numbered among themselves.
    calls: HashMap<u32, u32>,
    usage: Usage,
    stop: Option<FinishReason>,
    done: bool,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    MessageStart {
        message: StartedMessage,
    },
    ContentBlockStart {
        index: u32,
        content_block: Value,
    },
    ContentBlockDelta {
        index: u32,
        delta: Value,
    },
    MessageDelta {
        delta: Value,
        usage: Option<WireUsage>,
    },
    MessageStop,
    Error {
        error: Value,
    },
    /// Pings, block ends and event types added later carry nothing this client needs.
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct StartedMessage {
    usage: Option<WireUsage>,
}

#[derive(Deserialize, Default)]
struct WireUsage {
    input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

impl MessageParser {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<ModelEvent>> {
        let mut events = Vec::new();
        for data in self.decoder.feed(bytes)? {
            if self.done {
                break;
            }
            events.extend(self.parse_event(&data)?);
        }
        Ok(events)
    }

    /// EOF before `message_stop` is a cut stream.
    fn finish(&mut self) -> Result<()> {
        if !self.done {
            self.done = true;
            bail!(Transient(
                "provider stream ended before the response was complete".into()
            ));
        }
        Ok(())
    }

    fn parse_event(&mut self, data: &str) -> Result<Vec<ModelEvent>> {
        // Upstream payloads can echo input; never copy them into errors.
        let event: Event = serde_json::from_str(data)
            .map_err(|_| anyhow!("invalid provider stream event JSON"))?;
        let mut events = Vec::new();
        match event {
            Event::MessageStart { message } => self.count(message.usage.unwrap_or_default()),
            Event::ContentBlockStart {
                index,
                content_block: block,
            } => match block["type"].as_str() {
                Some("tool_use") => {
                    let call = self.calls.len() as u32;
                    self.calls.insert(index, call);
                    events.push(ModelEvent::ToolCall {
                        index: call,
                        id: field(&block, "id"),
                        name: field(&block, "name"),
                    });
                }
                Some("text") => {
                    events.extend(non_empty(field(&block, "text")).map(ModelEvent::Text))
                }
                Some("thinking") => {
                    events.extend(non_empty(field(&block, "thinking")).map(ModelEvent::Reasoning));
                }
                _ => {}
            },
            Event::ContentBlockDelta { index, delta } => match delta["type"].as_str() {
                Some("text_delta") => {
                    events.extend(non_empty(field(&delta, "text")).map(ModelEvent::Text));
                }
                Some("thinking_delta") => {
                    events.extend(non_empty(field(&delta, "thinking")).map(ModelEvent::Reasoning));
                }
                Some("input_json_delta") => {
                    let call = *self
                        .calls
                        .get(&index)
                        .ok_or_else(|| anyhow!("tool input for a block that is not a call"))?;
                    events.extend(non_empty(field(&delta, "partial_json")).map(|fragment| {
                        ModelEvent::ToolArguments {
                            index: call,
                            fragment,
                        }
                    }));
                }
                _ => {}
            },
            Event::MessageDelta { delta, usage } => {
                if let Some(reason) = delta["stop_reason"].as_str() {
                    self.stop = Some(finish_reason(reason)?);
                }
                self.count(usage.unwrap_or_default());
            }
            Event::MessageStop => {
                self.done = true;
                events.push(ModelEvent::Usage(self.usage));
                events.push(ModelEvent::Finished(
                    self.stop.unwrap_or(FinishReason::Stop),
                ));
            }
            Event::Error { error } => {
                if matches!(
                    error["type"].as_str(),
                    Some("overloaded_error" | "api_error" | "rate_limit_error")
                ) {
                    bail!(Transient("provider is overloaded".into()));
                }
                bail!("provider reported an error during streaming");
            }
            Event::Other => {}
        }
        Ok(events)
    }

    /// `message_start` brings the input, `message_delta` the output so far.
    fn count(&mut self, usage: WireUsage) {
        if let Some(input) = usage.input_tokens {
            let cached = usage.cache_read_input_tokens.unwrap_or(0);
            let written = usage.cache_creation_input_tokens.unwrap_or(0);
            self.usage.prompt_tokens = input + cached + written;
            self.usage.cached_prompt_tokens = cached;
        }
        if let Some(output) = usage.output_tokens {
            self.usage.completion_tokens = output;
        }
    }
}

fn field(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

fn finish_reason(reason: &str) -> Result<FinishReason> {
    Ok(match reason {
        "end_turn" | "stop_sequence" | "pause_turn" => FinishReason::Stop,
        "tool_use" => FinishReason::ToolCalls,
        "max_tokens" | "model_context_window_exceeded" => FinishReason::Length,
        "refusal" => FinishReason::ContentFilter,
        _ => bail!("model stopped for an unsupported reason"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{Picture, is_transient};

    fn event(data: &str) -> String {
        format!("data: {data}\n\n")
    }

    #[test]
    fn a_stream_becomes_reasoning_text_calls_and_usage() {
        let stream = [
            r#"{"type":"message_start","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":90,"output_tokens":1}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"x"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"ping"}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Hi"}}"#,
            r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_1","name":"read_file","input":{}}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#,
            r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\"a\"}"}}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":42}}"#,
            r#"{"type":"message_stop"}"#,
        ]
        .map(event)
        .concat();
        let mut parser = MessageParser::default();
        let events = parser.feed(stream.as_bytes()).unwrap();
        assert!(parser.done);
        assert_eq!(
            events,
            vec![
                ModelEvent::Reasoning("hmm".into()),
                ModelEvent::Text("Hi".into()),
                ModelEvent::ToolCall {
                    index: 0,
                    id: "toolu_1".into(),
                    name: "read_file".into()
                },
                ModelEvent::ToolArguments {
                    index: 0,
                    fragment: "{\"path\":".into()
                },
                ModelEvent::ToolArguments {
                    index: 0,
                    fragment: "\"a\"}".into()
                },
                ModelEvent::Usage(Usage {
                    prompt_tokens: 100,
                    cached_prompt_tokens: 90,
                    completion_tokens: 42,
                    reasoning_tokens: 0,
                }),
                ModelEvent::Finished(FinishReason::ToolCalls),
            ]
        );
    }

    #[test]
    fn a_cut_stream_and_an_overload_may_pass_later() {
        let mut parser = MessageParser::default();
        parser
            .feed(event(r#"{"type":"message_start","message":{}}"#).as_bytes())
            .unwrap();
        assert!(is_transient(&parser.finish().unwrap_err()));
        let overloaded = event(r#"{"type":"error","error":{"type":"overloaded_error"}}"#);
        let error = MessageParser::default()
            .feed(overloaded.as_bytes())
            .unwrap_err();
        assert!(is_transient(&error));
    }

    #[test]
    fn history_becomes_alternating_turns_with_a_system_prompt() {
        let call = ToolCall {
            id: "toolu_1".into(),
            name: "read_file".into(),
            arguments: json!({"path": "a"}),
        };
        let messages = [
            ChatMessage::System("be brief".into()),
            ChatMessage::User("look".into()),
            ChatMessage::Assistant {
                text: String::new(),
                tool_calls: vec![call],
            },
            ChatMessage::Tool {
                call_id: "toolu_1".into(),
                content: "text".into(),
            },
            ChatMessage::UserWithPictures {
                text: "[sources]".into(),
                pictures: vec![Picture {
                    mime_type: "image/png",
                    base64: "AA==".into(),
                }],
            },
            ChatMessage::System("summarize".into()),
        ];
        let (system, wire) = wire_messages(&messages);
        assert_eq!(system, "be brief");
        assert_eq!(
            serde_json::to_value(&wire).unwrap(),
            json!([
                {"role": "user", "content": [{"type": "text", "text": "look"}]},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "toolu_1", "name": "read_file", "input": {"path": "a"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_1", "content": "text"},
                    {"type": "text", "text": "[sources]"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AA=="}},
                    {"type": "text", "text": "[system] summarize"}
                ]}
            ])
        );
    }
}
