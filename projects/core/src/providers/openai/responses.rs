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
use dusk_protocol::client::CHATGPT_USAGE_URL;
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    config::{Effort, ProviderConfig},
    providers::{
        errors::{ProviderError, ProviderErrorKind, UnsupportedPictures, ended_early},
        limits::{Account, Headers, PlanUse},
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
const SIGN_IN_AGAIN: &str =
    "ChatGPT did not accept the sign-in; sign in again in the provider settings";

pub struct ChatGpt {
    client: Client,
    models_url: Url,
    responses_url: Url,
    /// None when the record of the sign-in cannot be read, as with a locked keychain:
    /// the provider then says so when asked, and the others go on working.
    session: Option<Arc<Session>>,
    account: Account,
}

#[derive(Deserialize)]
struct Catalogue {
    /// The plan's list; `data` is how the same endpoint answers an API key.
    #[serde(default, alias = "data")]
    models: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    #[serde(alias = "id")]
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
            session: Session::of(&config.api_key.0).ok(),
            account: Account::of(config),
        })
    }

    async fn bearer(&self, refused: Option<&str>) -> Result<(String, HeaderValue)> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow!("{SIGN_IN_AGAIN}"))?;
        let token = session.access_token(&self.client, refused).await?;
        let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| anyhow!("the ChatGPT sign-in gave an unusable token"))?;
        value.set_sensitive(true);
        Ok((token, value))
    }

    /// Sends the request made by `build`, once more with a renewed token when the first
    /// one was refused as unauthorized.
    async fn send(
        &self,
        build: impl Fn() -> reqwest::RequestBuilder,
        failed: impl Fn(reqwest::Error) -> anyhow::Error,
    ) -> Result<Response> {
        let mut refused: Option<String> = None;
        loop {
            let (token, bearer) = self.bearer(refused.as_deref()).await?;
            let response = build()
                .header(AUTHORIZATION, bearer)
                .send()
                .await
                .map_err(&failed)?;
            if response.status() != StatusCode::UNAUTHORIZED || refused.is_some() {
                return Ok(response);
            }
            refused = Some(token);
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
                |_| anyhow!("provider model request failed"),
            )
            .await?;
        self.account.note(Headers::ChatGpt, response.headers());
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
                |error| http::unsent(error).into(),
            )
            .await?;
        self.account.note(Headers::ChatGpt, response.headers());
        let status = response.status();
        if !status.is_success() {
            // A missing service comes back; a spent plan does not until the user acts.
            let failure = http::status_failure(&response, http::refused("completion", status));
            if failure.kind.is_retryable() && status != StatusCode::TOO_MANY_REQUESTS {
                bail!(failure);
            }
            if has_pictures && matches!(status.as_u16(), 413 | 415 | 422) {
                bail!(UnsupportedPictures);
            }
            bail!(refusal(response, "completion").await);
        }
        // The documentation names the events, not the content type: anything but JSON is
        // read as events, and JSON is an answer that says why there is no stream.
        if !http::is_event_stream(&response) {
            let kind = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("none")
                .to_owned();
            // ChatGPT names no type for its stream, so only another type is worth a note.
            if kind != "none" {
                tracing::warn!(content_type = %kind, "ChatGPT answered without an event stream");
            }
            if kind.starts_with("application/json") {
                bail!(refusal(response, "completion").await);
            }
        }
        Ok(ResponseStream {
            response,
            account: self.account,
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
async fn refusal(response: Response, what: &str) -> ProviderError {
    let status = response.status();
    let error = tokio::time::timeout(ERROR_BODY_TIMEOUT, body(response))
        .await
        .ok()
        .and_then(Result::ok)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|body| body.get("error").cloned())
        .unwrap_or_default();
    let field = |name: &str| error.get(name).and_then(Value::as_str).unwrap_or_default();
    // A spent plan answers 429 and does not come back until the user acts.
    let kind = match status {
        StatusCode::TOO_MANY_REQUESTS => ProviderErrorKind::Quota,
        status => http::status_kind(status),
    };
    let fallback = match status {
        // There is no key to change here: the token was renewed once and refused again.
        StatusCode::UNAUTHORIZED => SIGN_IN_AGAIN.to_owned(),
        // The server's own words, short and printable, say more than the status.
        status if !field("message").is_empty() => {
            let message: String = field("message")
                .chars()
                .filter(|c| !c.is_control())
                .take(200)
                .collect();
            format!("{}: {message}", http::refused(what, status))
        }
        status => http::refused(what, status),
    };
    let (kind, message) = said(field("code")).unwrap_or((kind, fallback));
    ProviderError {
        status: Some(status.as_u16()),
        ..ProviderError::new(kind, message)
    }
}

/// The plan's own refusals, in words that tell the user what to do.
fn said(code: &str) -> Option<(ProviderErrorKind, String)> {
    use ProviderErrorKind::*;
    Some(match code {
        "subscription_sharing_usage_limit_exceeded" => (
            Quota,
            format!("the ChatGPT plan's limit for apps is used up; see {CHATGPT_USAGE_URL}"),
        ),
        "usage_limit_reached" | "rate_limit_exceeded" | "insufficient_quota" => (
            Quota,
            format!("the ChatGPT plan's usage limit is reached; see {CHATGPT_USAGE_URL}"),
        ),
        "usage_not_included" => (
            Quota,
            format!("this ChatGPT plan does not include use in apps; see {CHATGPT_USAGE_URL}"),
        ),
        "context_length_exceeded" => (
            TooLong,
            "the chat is longer than the model takes; /compact makes it shorter".to_owned(),
        ),
        "subscription_sharing_user_not_eligible" => (
            Auth,
            "this ChatGPT account may not use its plan in apps".to_owned(),
        ),
        "subscription_sharing_unsupported_capability" => (
            Refused,
            "the ChatGPT plan does not take this model or one of the request's features".to_owned(),
        ),
        "subscription_sharing_invalid_user"
        | "chatpass_v2_scope_not_authorized"
        | "chatpass_v2_invalid_authorization_context" => (Auth, SIGN_IN_AGAIN.to_owned()),
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
    // Keeps a thread's steps on the machine that holds its cached prefix: threads share
    // the start of their prompt, and without a key they push each other off it.
    if let Some(key) = request.cache_key {
        wire["prompt_cache_key"] = key.into();
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
    /// Where the plan's windows the stream reports are kept.
    account: Account,
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
            let chunk = self.response.chunk().await.map_err(http::cut)?;
            match chunk {
                Some(bytes) => {
                    let parsed = self.parser.feed(&bytes);
                    self.parser.note_plan(self.account);
                    self.pending.extend(parsed?);
                    self.finished = self.parser.done;
                }
                None => bail!(ended_early()),
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
    /// The tool calls counted; the map may not grow when a stream repeats a place.
    counted: u32,
    /// The plan's windows, when the stream reported them.
    plan: Vec<[Option<PlanUse>; 2]>,
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
    error: Option<Reason>,
    /// Read apart, so a malformed window cannot interrupt the answer.
    rate_limits: Option<Value>,
    metered_limit_name: Option<Value>,
    limit_name: Option<Value>,
}

/// The provider's slots do not imply a fixed duration. A malformed window is ignored.
#[derive(Deserialize)]
struct PlanWindows {
    #[serde(default, deserialize_with = "lenient")]
    primary: Option<PlanUse>,
    #[serde(default, deserialize_with = "lenient")]
    secondary: Option<PlanUse>,
}

fn lenient<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<PlanUse>, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
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
    cache_write_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct OutputDetails {
    reasoning_tokens: Option<u64>,
}

impl Parser {
    fn note_plan(&mut self, account: Account) {
        for windows in self.plan.drain(..) {
            account.note_plan(windows);
        }
    }

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
                if let Some(item) = event.item.filter(|item| item.r#type == "function_call")
                    && !self.calls.contains_key(&event.output_index)
                {
                    let index = self.counted;
                    self.counted += 1;
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
            // ChatGPT says how much of the plan is used as an event of its own.
            "codex.rate_limits" => {
                let bucket = event
                    .metered_limit_name
                    .as_ref()
                    .or(event.limit_name.as_ref());
                if bucket.is_some_and(|bucket| {
                    !bucket
                        .as_str()
                        .is_some_and(|name| name.trim().eq_ignore_ascii_case("codex"))
                }) {
                    return Ok(());
                }
                let windows = event
                    .rate_limits
                    .and_then(|value| serde_json::from_value::<PlanWindows>(value).ok());
                if let Some(windows) = windows {
                    self.plan.push([windows.primary, windows.secondary]);
                }
            }
            "response.failed" | "error" => {
                self.done = true;
                let code = event
                    .response
                    .and_then(|response| response.error)
                    .and_then(|error| error.code)
                    .or(event.error.and_then(|error| error.code))
                    .or(event.code)
                    .unwrap_or_default();
                // The code alone: the message can echo the input.
                let shown: String = code
                    .chars()
                    .filter(char::is_ascii_graphic)
                    .take(64)
                    .collect();
                tracing::warn!(code = %shown, "ChatGPT ended the stream with an error");
                return Err(match said(&code) {
                    Some((kind, message)) => ProviderError::new(kind, message).into(),
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
            let input = usage.input_tokens_details.as_ref();
            events.push(ModelEvent::Usage(Usage {
                prompt_tokens: usage.input_tokens,
                cached_prompt_tokens: input.and_then(|details| details.cached_tokens).unwrap_or(0),
                cache_write_tokens: input
                    .and_then(|details| details.cache_write_tokens)
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
    fn a_used_up_plan_says_so_and_is_not_retried() {
        for event in [
            json!({"type":"response.failed","response":{"error":{"code":"usage_limit_reached"}}}),
            json!({"type":"error","error":{"code":"usage_limit_reached","message":"echo"}}),
        ] {
            let error = Parser::default().feed(&stream(&[event])).unwrap_err();
            assert!(error.to_string().contains("usage limit"), "{error}");
            assert!(!crate::providers::is_transient(&error));
        }
    }

    #[test]
    fn the_plan_windows_of_a_stream_are_read_and_a_strange_one_is_left_out() {
        let body = stream(&[
            json!({"type":"codex.rate_limits","plan_type":"plus","rate_limits":{
                "primary":{"used_percent":12.5,"window_minutes":300,"reset_at":1},
                "secondary":{"used_percent":40,"window_minutes":10080,"reset_at":2}}}),
            json!({"type":"response.output_text.delta","delta":"Hi"}),
        ]);
        let mut parser = Parser::default();
        let events = parser.feed(&body).unwrap();
        assert_eq!(events, [ModelEvent::Text("Hi".into())]);
        let [[Some(primary), Some(secondary)]] = parser.plan.as_slice() else {
            panic!("expected two windows");
        };
        assert_eq!(
            (primary.window_minutes, primary.used_percent),
            (Some(300), 12.5)
        );
        assert_eq!(
            (secondary.window_minutes, secondary.used_percent),
            (Some(10080), 40.0)
        );

        let odd = stream(&[json!({"type":"codex.rate_limits","rate_limits":{
            "primary":{"used_percent":"lots"},"secondary":{"used_percent":3}}})]);
        let mut parser = Parser::default();
        assert!(parser.feed(&odd).unwrap().is_empty());
        assert!(matches!(parser.plan.as_slice(), [[None, Some(_)]]));
    }

    #[tokio::test]
    async fn authorized_stream_keeps_partial_plan_windows_even_when_inference_fails() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let fixture = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut request = String::new();
            loop {
                let mut line = String::new();
                assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                request.push_str(&line);
            }
            assert!(request.starts_with("POST /v1/responses "));
            assert!(request.contains("authorization: Bearer fixture-quota-token\r\n"));
            let body = stream(&[
                json!({"type":"codex.rate_limits","rate_limits":{
                    "secondary":{"used_percent":74,"window_minutes":10080}}}),
                json!({"type":"codex.rate_limits","metered_limit_name":"code-review","rate_limits":{
                    "primary":{"used_percent":99,"window_minutes":300}}}),
                json!({"type":"codex.rate_limits","limit_name":"  CoDeX  ","rate_limits":{
                    "primary":{"used_percent":21}}}),
                json!({"type":"response.failed","response":{"error":{
                    "code":"subscription_sharing_usage_limit_exceeded"}}}),
            ]);
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\
                 x-codex-primary-used-percent: 18\r\nx-codex-primary-window-minutes: 300\r\n\r\n",
                body.len()
            );
            socket
                .get_mut()
                .write_all(headers.as_bytes())
                .await
                .unwrap();
            socket.get_mut().write_all(&body).await.unwrap();
            socket.get_mut().shutdown().await.unwrap();
        });
        let record = json!({
            "client_id":"fixture-authorized-quota-stream","host_id":"fixture","subject":"fixture",
            "email":null,"access_token":"fixture-quota-token","refresh_token":"",
            "expires_at":i64::MAX,"saved_at":1,
        });
        let config: ProviderConfig = serde_json::from_value(json!({
            "base_url":super::super::sign_in::RESOURCE,
            "kind":"chatgpt","api_key":record.to_string(),
        }))
        .unwrap();
        let same_account = Account::of(&config);
        assert!(same_account.limits().is_none());
        let mut provider = ChatGpt::new(&config).unwrap();
        provider.responses_url = format!("http://{addr}/v1/responses").parse().unwrap();
        let request = ChatRequest {
            model: "fixture",
            messages: &[ChatMessage::User("fixture".into())],
            tools: &[],
            max_output_tokens: None,
            effort: None,
            cache_key: None,
        };
        let mut stream = provider.stream_chat(request).await.unwrap();
        assert!(stream.next_event().await.is_err());
        fixture.await.unwrap();
        let plan = same_account.limits().unwrap().plan;
        assert_eq!(
            plan.iter()
                .map(|p| (p.minutes, p.used_percent))
                .collect::<Vec<_>>(),
            [(Some(300), 21), (Some(10_080), 74)]
        );
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
    fn a_stream_preserves_reported_cache_writes() {
        let body = stream(&[json!({
            "type": "response.completed",
            "response": {"usage": {
                "input_tokens": 12000,
                "input_tokens_details": {"cached_tokens": 4000, "cache_write_tokens": 7000},
                "output_tokens": 2
            }}
        })]);
        assert_eq!(
            Parser::default().feed(&body).unwrap(),
            [
                ModelEvent::Finished(FinishReason::Stop),
                ModelEvent::Usage(Usage {
                    prompt_tokens: 12000,
                    cached_prompt_tokens: 4000,
                    cache_write_tokens: 7000,
                    completion_tokens: 2,
                    ..Usage::default()
                }),
            ],
        );
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
            error.contains(CHATGPT_USAGE_URL) && !error.contains("echo"),
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
        assert_eq!(wire["prompt_cache_key"], "thread");
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
