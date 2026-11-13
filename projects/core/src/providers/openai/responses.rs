//! The Responses API on a ChatGPT plan: what a sign-in with ChatGPT may ask. The whole
//! history goes with every request, nothing is stored at OpenAI (`store: false`), and
//! the answer is always a stream. The route refuses `max_output_tokens`, `temperature`
//! and system messages; the system prompt goes as `instructions`.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use anyhow::{Result, anyhow, bail, ensure};
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{ACCEPT, AUTHORIZATION, HeaderValue},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    config::{Effort, ProviderConfig},
    providers::{
        errors::{Transient, UnsupportedPictures},
        limits::{Account, Headers},
        output::{FinishReason, ModelEvent, Usage},
        registry::Model,
        request::{ChatMessage, ChatRequest, Picture},
        transport::{http, sse::SseDecoder},
    },
};

use super::sign_in::Session;

const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const CATALOGUE_TIMEOUT: Duration = Duration::from_secs(30);
const ERROR_BODY_TIMEOUT: Duration = Duration::from_secs(5);
/// Where the user sees and sets how much of the plan an app may use.
const USAGE_PAGE: &str = "https://chatgpt.com/settings/usage";

pub struct ChatGpt {
    client: Client,
    models_url: Url,
    responses_url: Url,
    session: Arc<Session>,
    account: Account,
}

#[derive(Deserialize)]
struct Catalogue {
    #[serde(default)]
    models: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    slug: String,
    visibility: Option<String>,
    context_window: Option<u64>,
}

impl ChatGpt {
    /// `config.api_key` holds the sign-in record, not a key.
    pub fn new(config: &ProviderConfig) -> Result<Self> {
        // The tokens are for OpenAI's API alone; a changed address would send them elsewhere.
        ensure!(
            config.base_url.trim_end_matches('/') == super::sign_in::RESOURCE,
            "a ChatGPT sign-in works only with {}",
            super::sign_in::RESOURCE
        );
        let base_url = http::base_url(&config.base_url, false)?;
        Ok(Self {
            client: http::client()?,
            models_url: http::endpoint(&base_url, &["models"])?,
            responses_url: http::endpoint(&base_url, &["responses"])?,
            session: Session::of(&config.api_key.0)?,
            account: Account::of(config),
        })
    }

    async fn bearer(&self, refused: bool) -> Result<HeaderValue> {
        let token = self.session.access_token(&self.client, refused).await?;
        let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| anyhow!("the ChatGPT sign-in gave an unusable token"))?;
        value.set_sensitive(true);
        Ok(value)
    }

    /// Sends the request made by `build`, once more with a renewed token when the first
    /// one was refused as unauthorized.
    async fn send(
        &self,
        build: impl Fn() -> reqwest::RequestBuilder,
        failed: impl Fn() -> anyhow::Error,
    ) -> Result<Response> {
        let mut refused = false;
        loop {
            let response = build()
                .header(AUTHORIZATION, self.bearer(refused).await?)
                .send()
                .await
                .map_err(|_| failed())?;
            if response.status() != StatusCode::UNAUTHORIZED || refused {
                return Ok(response);
            }
            refused = true;
        }
    }

    pub async fn list_models(&self) -> Result<Vec<Model>> {
        let response = self
            .send(
                || {
                    self.client
                        .get(self.models_url.clone())
                        .header(ACCEPT, "application/json")
                        .timeout(CATALOGUE_TIMEOUT)
                },
                || anyhow!("provider model request failed"),
            )
            .await?;
        self.account.note(Headers::OpenAi, response.headers());
        let status = response.status();
        if !status.is_success() {
            bail!(refusal(response, "model").await);
        }
        let catalogue: Catalogue = serde_json::from_slice(&body(response).await?)
            .map_err(|_| anyhow!("invalid provider model catalogue JSON"))?;
        let mut models = BTreeMap::new();
        for model in catalogue.models {
            // Only what the account's own picker lists.
            if model
                .visibility
                .as_deref()
                .is_some_and(|shown| shown != "list")
            {
                continue;
            }
            ensure!(
                !model.slug.trim().is_empty() && !model.slug.chars().any(char::is_control),
                "provider returned an empty or invalid model ID"
            );
            models.insert(model.slug, model.context_window);
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

    pub async fn stream_chat(&self, request: ChatRequest<'_>) -> Result<ResponseStream> {
        let has_pictures = request.messages.iter().any(|message| {
            matches!(message, ChatMessage::UserWithPictures { .. })
                || matches!(message, ChatMessage::Tool { pictures, .. } if !pictures.is_empty())
        });
        let wire = wire_request(&request);
        let response = self
            .send(
                || {
                    self.client
                        .post(self.responses_url.clone())
                        .header(ACCEPT, http::EVENT_STREAM)
                        .json(&wire)
                },
                || anyhow!(Transient("provider completion request failed".into())),
            )
            .await?;
        self.account.note(Headers::OpenAi, response.headers());
        let status = response.status();
        if !status.is_success() {
            // A missing service comes back; a spent plan does not until the user acts.
            if http::is_retryable(status) && status != StatusCode::TOO_MANY_REQUESTS {
                let message = http::refused("completion", status);
                return Err(http::retry_failure(&response, message));
            }
            if has_pictures && matches!(status.as_u16(), 413 | 415 | 422) {
                bail!(UnsupportedPictures);
            }
            bail!(refusal(response, "completion").await);
        }
        ensure!(
            http::is_event_stream(&response),
            "provider did not return a stream"
        );
        Ok(ResponseStream {
            response,
            parser: Parser::default(),
            pending: VecDeque::new(),
            finished: false,
        })
    }
}

async fn body(mut response: Response) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("cannot read the provider's answer"))?
    {
        ensure!(
            chunk.len() <= MAX_BODY_BYTES - bytes.len(),
            "the provider's answer is too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Why a request was refused, from the error's code alone: its message can echo input.
async fn refusal(response: Response, what: &str) -> String {
    let status = response.status();
    let code = tokio::time::timeout(ERROR_BODY_TIMEOUT, body(response))
        .await
        .ok()
        .and_then(Result::ok)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|body| Some(body.get("error")?.get("code")?.as_str()?.to_owned()))
        .unwrap_or_default();
    said(&code).unwrap_or_else(|| http::refused(what, status))
}

/// The plan's own refusals, in words that tell the user what to do.
fn said(code: &str) -> Option<String> {
    Some(match code {
        "subscription_sharing_usage_limit_exceeded" => {
            format!("the ChatGPT plan's limit for apps is used up; see {USAGE_PAGE}")
        }
        "subscription_sharing_user_not_eligible" => {
            "this ChatGPT account may not use its plan in apps".to_owned()
        }
        "subscription_sharing_unsupported_capability" => {
            "the ChatGPT plan does not take this model or one of the request's features".to_owned()
        }
        "subscription_sharing_invalid_user"
        | "chatpass_v2_scope_not_authorized"
        | "chatpass_v2_invalid_authorization_context" => {
            "ChatGPT did not accept the sign-in; sign in again in the provider settings".to_owned()
        }
        _ => return None,
    })
}

fn wire_request(request: &ChatRequest<'_>) -> Value {
    let mut instructions = String::new();
    let mut input = Vec::new();
    for message in request.messages {
        match message {
            // The first system text is the instructions; a later one, such as a summary
            // of earlier turns, speaks as the developer: system items are refused.
            ChatMessage::System(text) if input.is_empty() && instructions.is_empty() => {
                instructions = text.clone();
            }
            ChatMessage::System(text) => input.push(spoken("developer", text, &[])),
            ChatMessage::User(text) => input.push(spoken("user", text, &[])),
            ChatMessage::UserWithPictures { text, pictures } => {
                input.push(spoken("user", text, pictures));
            }
            ChatMessage::Assistant { text, tool_calls } => {
                if !text.is_empty() {
                    input.push(json!({
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": text }],
                    }));
                }
                for call in tool_calls {
                    let arguments = match &call.arguments {
                        Value::String(raw) => raw.clone(),
                        arguments => arguments.to_string(),
                    };
                    input.push(json!({
                        "type": "function_call",
                        "call_id": call.id,
                        "name": call.name,
                        "arguments": arguments,
                    }));
                }
            }
            ChatMessage::Tool {
                call_id,
                content,
                pictures,
            } => {
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": content,
                }));
                if !pictures.is_empty() {
                    let note = "[the pictures the tool call above returned]";
                    input.push(spoken("user", note, pictures));
                }
            }
        }
    }
    let tools: Vec<Value> = request
        .tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.parameters,
                "strict": false,
            })
        })
        .collect();
    let mut wire = json!({
        "model": request.model,
        "input": input,
        "store": false,
        "stream": true,
    });
    if !instructions.is_empty() {
        wire["instructions"] = instructions.into();
    }
    if !tools.is_empty() {
        wire["tools"] = tools.into();
    }
    if let Some(effort) = request.effort.filter(|effort| *effort != Effort::Off) {
        wire["reasoning"] = json!({ "effort": effort.wire_name(), "summary": "auto" });
    }
    wire
}

fn spoken(role: &str, text: &str, pictures: &[Picture]) -> Value {
    let mut content = vec![json!({ "type": "input_text", "text": text })];
    content.extend(pictures.iter().map(|picture| {
        json!({
            "type": "input_image",
            "image_url": format!("data:{};base64,{}", picture.mime_type, picture.base64),
        })
    }));
    json!({ "role": role, "content": content })
}

pub struct ResponseStream {
    response: Response,
    parser: Parser,
    pending: VecDeque<ModelEvent>,
    finished: bool,
}

impl ResponseStream {
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
                None => bail!(Transient(
                    "provider stream ended before the response was complete".into()
                )),
            }
        }
    }
}

#[derive(Default)]
struct Parser {
    decoder: SseDecoder,
    /// The tool calls so far, by the place of their item in the output: the number the
    /// agent knows them by, and whether their arguments came in pieces.
    calls: HashMap<u64, (u32, bool)>,
    done: bool,
}

#[derive(Deserialize)]
struct Event {
    r#type: String,
    #[serde(default)]
    output_index: u64,
    delta: Option<String>,
    item: Option<Item>,
    response: Option<Finished>,
    code: Option<String>,
}

#[derive(Deserialize)]
struct Item {
    r#type: String,
    call_id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct Finished {
    usage: Option<WireUsage>,
    incomplete_details: Option<Reason>,
    error: Option<Reason>,
}

#[derive(Deserialize)]
struct Reason {
    reason: Option<String>,
    code: Option<String>,
}

#[derive(Deserialize)]
struct WireUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    input_tokens_details: Option<InputDetails>,
    output_tokens_details: Option<OutputDetails>,
}

#[derive(Deserialize)]
struct InputDetails {
    cached_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct OutputDetails {
    reasoning_tokens: Option<u64>,
}

impl Parser {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<ModelEvent>> {
        let mut events = Vec::new();
        for data in self.decoder.feed(bytes)? {
            if self.done {
                break;
            }
            // Upstream payloads can echo input; never copy them into errors.
            let event: Event = serde_json::from_str(&data)
                .map_err(|_| anyhow!("invalid provider stream event JSON"))?;
            self.take(event, &mut events)?;
        }
        Ok(events)
    }

    fn take(&mut self, event: Event, events: &mut Vec<ModelEvent>) -> Result<()> {
        let text = |delta: Option<String>| delta.filter(|delta| !delta.is_empty());
        match event.r#type.as_str() {
            "response.output_text.delta" | "response.refusal.delta" => {
                events.extend(text(event.delta).map(ModelEvent::Text));
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                events.extend(text(event.delta).map(ModelEvent::Reasoning));
            }
            // One part of the summary ended; the next starts a new paragraph.
            "response.reasoning_summary_part.done" => {
                events.push(ModelEvent::Reasoning("\n\n".into()));
            }
            "response.output_item.added" => {
                if let Some(item) = event.item.filter(|item| item.r#type == "function_call") {
                    let index = self.calls.len() as u32;
                    self.calls.insert(event.output_index, (index, false));
                    events.push(ModelEvent::ToolCall {
                        index,
                        id: item.call_id.unwrap_or_else(|| format!("call_{index}")),
                        name: item.name.unwrap_or_default(),
                    });
                }
            }
            "response.function_call_arguments.delta" => {
                if let (Some((index, streamed)), Some(fragment)) =
                    (self.calls.get_mut(&event.output_index), text(event.delta))
                {
                    *streamed = true;
                    events.push(ModelEvent::ToolArguments {
                        index: *index,
                        fragment,
                    });
                }
            }
            // A call whose arguments came whole, with the finished item.
            "response.output_item.done" => {
                if let (Some((index, false)), Some(fragment)) = (
                    self.calls.get(&event.output_index).copied(),
                    text(event.item.and_then(|item| item.arguments)),
                ) {
                    events.push(ModelEvent::ToolArguments { index, fragment });
                }
            }
            "response.completed" => {
                let reason = match self.calls.is_empty() {
                    true => FinishReason::Stop,
                    false => FinishReason::ToolCalls,
                };
                self.finish(event.response, reason, events);
            }
            "response.incomplete" => {
                let why = event
                    .response
                    .as_ref()
                    .and_then(|response| response.incomplete_details.as_ref())
                    .and_then(|details| details.reason.clone())
                    .unwrap_or_default();
                let reason = match why.as_str() {
                    "content_filter" => FinishReason::ContentFilter,
                    _ => FinishReason::Length,
                };
                self.finish(event.response, reason, events);
            }
            "response.failed" | "error" => {
                self.done = true;
                let code = event
                    .response
                    .and_then(|response| response.error)
                    .and_then(|error| error.code)
                    .or(event.code)
                    .unwrap_or_default();
                return Err(match said(&code) {
                    Some(message) => anyhow!(message),
                    None => crate::providers::stream_error(&code),
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(
        &mut self,
        response: Option<Finished>,
        reason: FinishReason,
        events: &mut Vec<ModelEvent>,
    ) {
        self.done = true;
        events.push(ModelEvent::Finished(reason));
        if let Some(usage) = response.and_then(|response| response.usage) {
            events.push(ModelEvent::Usage(Usage {
                prompt_tokens: usage.input_tokens,
                cached_prompt_tokens: usage
                    .input_tokens_details
                    .and_then(|details| details.cached_tokens)
                    .unwrap_or(0),
                completion_tokens: usage.output_tokens,
                reasoning_tokens: usage
                    .output_tokens_details
                    .and_then(|details| details.reasoning_tokens)
                    .unwrap_or(0),
                ..Usage::default()
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{ToolCall, ToolSpec};

    fn stream(events: &[Value]) -> Vec<u8> {
        events
            .iter()
            .map(|event| {
                format!(
                    "event: {}\ndata: {event}\n\n",
                    event["type"].as_str().unwrap()
                )
            })
            .collect::<String>()
            .into_bytes()
    }

    #[test]
    fn a_stream_gives_thought_text_calls_and_usage_in_order() {
        let body = stream(&[
            json!({"type":"response.created","response":{}}),
            json!({"type":"response.reasoning_summary_text.delta","delta":"think"}),
            json!({"type":"response.output_text.delta","delta":"Hi"}),
            json!({"type":"response.output_item.added","output_index":2,
                "item":{"type":"function_call","call_id":"call_a","name":"weather","arguments":""}}),
            json!({"type":"response.function_call_arguments.delta","output_index":2,"delta":"{\"city\":"}),
            json!({"type":"response.function_call_arguments.delta","output_index":2,"delta":"\"Kazan\"}"}),
            json!({"type":"response.output_item.done","output_index":2,
                "item":{"type":"function_call","call_id":"call_a","name":"weather","arguments":"{\"city\":\"Kazan\"}"}}),
            json!({"type":"response.output_item.added","output_index":3,
                "item":{"type":"function_call","call_id":"call_b","name":"time"}}),
            json!({"type":"response.output_item.done","output_index":3,
                "item":{"type":"function_call","call_id":"call_b","name":"time","arguments":"{}"}}),
            json!({"type":"response.completed","response":{"usage":{"input_tokens":5,"output_tokens":2,
                "input_tokens_details":{"cached_tokens":3},"output_tokens_details":{"reasoning_tokens":1}}}}),
        ]);
        let mut parser = Parser::default();
        let events = parser.feed(&body).unwrap();
        let arguments = |index, fragment: &str| ModelEvent::ToolArguments {
            index,
            fragment: fragment.into(),
        };
        assert_eq!(
            events,
            [
                ModelEvent::Reasoning("think".into()),
                ModelEvent::Text("Hi".into()),
                ModelEvent::ToolCall {
                    index: 0,
                    id: "call_a".into(),
                    name: "weather".into()
                },
                arguments(0, "{\"city\":"),
                arguments(0, "\"Kazan\"}"),
                ModelEvent::ToolCall {
                    index: 1,
                    id: "call_b".into(),
                    name: "time".into()
                },
                arguments(1, "{}"),
                ModelEvent::Finished(FinishReason::ToolCalls),
                ModelEvent::Usage(Usage {
                    prompt_tokens: 5,
                    cached_prompt_tokens: 3,
                    completion_tokens: 2,
                    reasoning_tokens: 1,
                    ..Usage::default()
                }),
            ]
        );
        assert!(parser.done);
    }

    #[test]
    fn a_cut_answer_and_a_spent_plan_are_told_apart() {
        let cut = stream(&[json!({"type":"response.incomplete",
            "response":{"incomplete_details":{"reason":"max_output_tokens"}}})]);
        assert_eq!(
            Parser::default().feed(&cut).unwrap(),
            [ModelEvent::Finished(FinishReason::Length)]
        );
        let spent = stream(&[json!({"type":"response.failed","response":{"error":{
            "code":"subscription_sharing_usage_limit_exceeded","message":"echo of the input"}}})]);
        let error = Parser::default().feed(&spent).unwrap_err().to_string();
        assert!(
            error.contains(USAGE_PAGE) && !error.contains("echo"),
            "{error}"
        );
    }

    #[test]
    fn the_request_follows_the_rules_of_the_plan_route() {
        let messages = [
            ChatMessage::System("be brief".into()),
            ChatMessage::User("hi".into()),
            ChatMessage::Assistant {
                text: "calling".into(),
                tool_calls: vec![ToolCall {
                    id: "call_a".into(),
                    name: "weather".into(),
                    arguments: json!({"city":"Kazan"}),
                }],
            },
            ChatMessage::Tool {
                call_id: "call_a".into(),
                content: "sunny".into(),
                pictures: Vec::new(),
            },
            ChatMessage::System("summary of earlier turns".into()),
        ];
        let tools = [ToolSpec {
            name: "weather".into(),
            description: "the weather".into(),
            parameters: json!({"type":"object"}),
        }];
        let wire = wire_request(&ChatRequest {
            model: "gpt-x",
            messages: &messages,
            tools: &tools,
            max_output_tokens: Some(100),
            effort: Some(Effort::Off),
            cache_key: Some("thread"),
        });
        assert_eq!(wire["store"], false);
        assert_eq!(wire["stream"], true);
        assert_eq!(wire["instructions"], "be brief");
        // What the route refuses is never sent.
        for refused in [
            "max_output_tokens",
            "temperature",
            "previous_response_id",
            "reasoning",
        ] {
            assert!(wire.get(refused).is_none(), "{refused}");
        }
        let input = wire["input"].as_array().unwrap();
        assert_eq!(input.len(), 5);
        assert!(input.iter().all(|item| item["role"] != "system"));
        assert_eq!(input[2]["type"], "function_call");
        assert_eq!(input[2]["arguments"], "{\"city\":\"Kazan\"}");
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[4]["role"], "developer");
        assert_eq!(wire["tools"][0]["name"], "weather");
    }
}
