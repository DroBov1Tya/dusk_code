//! The streamed response, parsed from raw bytes into model events.

use std::{collections::VecDeque, sync::Arc};

use anyhow::{Result, anyhow, bail};
use reqwest::Response;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use super::signatures::Signatures;
use crate::providers::{
    errors::Transient,
    output::{FinishReason, ModelEvent, Usage},
    transport::sse::SseDecoder,
};

/// Events of one streamed response, read lazily from the network.
pub struct GenerateStream {
    response: Response,
    parser: GenerateParser,
    pending: VecDeque<ModelEvent>,
    finished: bool,
}

impl GenerateStream {
    pub fn new(response: Response, signatures: Arc<Signatures>) -> Self {
        Self {
            response,
            parser: GenerateParser::new(signatures),
            pending: VecDeque::new(),
            finished: false,
        }
    }

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
struct GenerateParser {
    decoder: SseDecoder,
    signatures: Arc<Signatures>,
    /// Function calls seen so far: the next one's index.
    calls: u32,
    usage: Usage,
    done: bool,
}

/// One `GenerateContentResponse` of the stream, or the error that ends it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Chunk {
    #[serde(default)]
    candidates: Vec<Candidate>,
    prompt_feedback: Option<PromptFeedback>,
    usage_metadata: Option<WireUsage>,
    error: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    content: Option<Content>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct Content {
    #[serde(default)]
    parts: Vec<Part>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Part {
    text: Option<String>,
    /// A summary of the model's thinking, not a part of its answer.
    #[serde(default)]
    thought: bool,
    function_call: Option<FunctionCall>,
    thought_signature: Option<String>,
}

#[derive(Deserialize)]
struct FunctionCall {
    id: Option<String>,
    #[serde(default)]
    name: String,
    args: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromptFeedback {
    block_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireUsage {
    #[serde(default)]
    prompt_token_count: u64,
    #[serde(default)]
    cached_content_token_count: u64,
    #[serde(default)]
    candidates_token_count: u64,
    #[serde(default)]
    tool_use_prompt_token_count: u64,
    #[serde(default)]
    thoughts_token_count: u64,
}

impl GenerateParser {
    fn new(signatures: Arc<Signatures>) -> Self {
        Self {
            decoder: SseDecoder::default(),
            signatures,
            calls: 0,
            usage: Usage::default(),
            done: false,
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<ModelEvent>> {
        let mut events = Vec::new();
        for data in self.decoder.feed(bytes)? {
            if self.done {
                break;
            }
            events.extend(self.parse_chunk(&data)?);
        }
        Ok(events)
    }

    /// The stream has no closing event: EOF before a finish reason is a cut stream.
    fn finish(&mut self) -> Result<()> {
        if !self.done {
            self.done = true;
            bail!(Transient(
                "provider stream ended before the response was complete".into()
            ));
        }
        Ok(())
    }

    fn parse_chunk(&mut self, data: &str) -> Result<Vec<ModelEvent>> {
        // Upstream payloads can echo input; never copy them into errors.
        let chunk: Chunk = serde_json::from_str(data)
            .map_err(|_| anyhow!("invalid provider stream event JSON"))?;
        if let Some(error) = chunk.error {
            let status = error["status"].as_str().unwrap_or_default();
            let code = error["code"].as_u64().unwrap_or_default();
            return Err(crate::providers::stream_error(&format!("{status} {code}")));
        }
        if let Some(usage) = chunk.usage_metadata {
            self.count(usage);
        }
        let mut events = Vec::new();
        let Some(candidate) = chunk.candidates.into_iter().next() else {
            // No candidate and a block reason: the prompt itself was refused.
            if chunk
                .prompt_feedback
                .is_some_and(|feedback| feedback.block_reason.is_some())
            {
                self.end(FinishReason::ContentFilter, &mut events);
            }
            return Ok(events);
        };
        for part in candidate
            .content
            .map(|content| content.parts)
            .unwrap_or_default()
        {
            if let Some(call) = part.function_call {
                // Only Gemini 3 and later always give an ID; results are matched by it.
                let id = call
                    .id
                    .filter(|id| !id.trim().is_empty())
                    .unwrap_or_else(|| format!("call_{}", Uuid::now_v7().simple()));
                if let Some(signature) = part.thought_signature {
                    self.signatures.remember(&id, signature);
                }
                let index = self.calls;
                self.calls += 1;
                events.push(ModelEvent::ToolCall {
                    index,
                    id,
                    name: call.name,
                });
                // Arguments come whole, as an object.
                if let Some(args) = call.args.filter(|args| !args.is_null()) {
                    events.push(ModelEvent::ToolArguments {
                        index,
                        fragment: args.to_string(),
                    });
                }
            } else if let Some(text) = part.text.filter(|text| !text.is_empty()) {
                events.push(match part.thought {
                    true => ModelEvent::Reasoning(text),
                    false => ModelEvent::Text(text),
                });
            }
        }
        if let Some(reason) = candidate.finish_reason {
            let reason = finish_reason(&reason, self.calls > 0)?;
            self.end(reason, &mut events);
        }
        Ok(events)
    }

    fn end(&mut self, reason: FinishReason, events: &mut Vec<ModelEvent>) {
        self.done = true;
        events.push(ModelEvent::Usage(self.usage));
        events.push(ModelEvent::Finished(reason));
    }

    /// Every chunk may report the usage so far; the last one is the whole. The prompt
    /// count includes the cached part, and thoughts are counted apart from the answer.
    fn count(&mut self, usage: WireUsage) {
        self.usage.prompt_tokens = usage.prompt_token_count + usage.tool_use_prompt_token_count;
        self.usage.cached_prompt_tokens = usage.cached_content_token_count;
        self.usage.completion_tokens = usage.candidates_token_count + usage.thoughts_token_count;
        self.usage.reasoning_tokens = usage.thoughts_token_count;
    }
}

/// A turn that called functions also ends with `STOP`.
fn finish_reason(reason: &str, called: bool) -> Result<FinishReason> {
    Ok(match reason {
        "STOP" if called => FinishReason::ToolCalls,
        "STOP" => FinishReason::Stop,
        "MAX_TOKENS" | "CONTINUATION" => FinishReason::Length,
        "SAFETY"
        | "RECITATION"
        | "LANGUAGE"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT"
        | "IMAGE_RECITATION" => FinishReason::ContentFilter,
        // The model slipped; the same request often passes when sent again.
        "MALFORMED_FUNCTION_CALL" | "MALFORMED_RESPONSE" | "OTHER" => {
            bail!(Transient("model answer was malformed".into()))
        }
        _ => bail!("model stopped for an unsupported reason"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::is_transient;

    fn event(data: &str) -> String {
        format!("data: {data}\r\n\r\n")
    }

    fn parser() -> GenerateParser {
        GenerateParser::new(Arc::default())
    }

    #[test]
    fn a_stream_becomes_reasoning_text_a_call_and_usage() {
        let stream = [
            r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"hmm","thought":true}]},"index":0}],"usageMetadata":{"promptTokenCount":100,"totalTokenCount":100},"modelVersion":"gemini-3-flash-preview","responseId":"r1"}"#,
            r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"Hi"}]},"index":0}]}"#,
            r#"{"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"read_file","args":{"path":"a"},"id":"fc_1"},"thoughtSignature":"SIG"},{"functionCall":{"name":"list"}}]},"finishReason":"STOP","index":0}],"usageMetadata":{"promptTokenCount":100,"cachedContentTokenCount":90,"candidatesTokenCount":30,"thoughtsTokenCount":12,"totalTokenCount":142}}"#,
        ]
        .map(event)
        .concat();
        let mut parser = parser();
        // Fed in pieces: an event may be cut anywhere.
        let (head, tail) = stream.as_bytes().split_at(stream.len() / 2);
        let mut events = parser.feed(head).unwrap();
        events.extend(parser.feed(tail).unwrap());
        assert!(parser.done);
        let ModelEvent::ToolCall { id: generated, .. } = &events[4] else {
            panic!("the second call is missing");
        };
        assert!(generated.starts_with("call_") && generated.len() > 20);
        assert_eq!(
            events,
            vec![
                ModelEvent::Reasoning("hmm".into()),
                ModelEvent::Text("Hi".into()),
                ModelEvent::ToolCall {
                    index: 0,
                    id: "fc_1".into(),
                    name: "read_file".into()
                },
                ModelEvent::ToolArguments {
                    index: 0,
                    fragment: r#"{"path":"a"}"#.into()
                },
                ModelEvent::ToolCall {
                    index: 1,
                    id: generated.clone(),
                    name: "list".into()
                },
                ModelEvent::Usage(Usage {
                    prompt_tokens: 100,
                    cached_prompt_tokens: 90,
                    completion_tokens: 42,
                    reasoning_tokens: 12,
                    ..Usage::default()
                }),
                ModelEvent::Finished(FinishReason::ToolCalls),
            ]
        );
        assert_eq!(parser.signatures.of("fc_1").as_deref(), Some("SIG"));
        assert_eq!(parser.signatures.of(generated), None);
    }

    #[test]
    fn broken_and_cut_streams_are_errors_not_panics() {
        for broken in [
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"te\n\n",
            "data: {\"candidates\":{}}\n\n",
            "data: [1,2]\n\n",
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":7}]}}]}\n\n",
            "data: \u{0}\n\n",
        ] {
            assert!(parser().feed(broken.as_bytes()).is_err(), "{broken}");
        }
        assert!(parser().feed(b"data: \xff\xfe\n\n").is_err());
        // Chunks this client has no use for are passed over.
        let mut parser = parser();
        let quiet = [
            r#"{}"#,
            r#"{"candidates":[{}]}"#,
            r#"{"candidates":[{"content":{}}]}"#,
        ];
        assert!(
            parser
                .feed(quiet.map(event).concat().as_bytes())
                .unwrap()
                .is_empty()
        );
        // Half an event, then the end.
        parser.feed(b"data: {\"candidates\":[{\"content\"").unwrap();
        assert!(is_transient(&parser.finish().unwrap_err()));
    }

    #[test]
    fn refusals_limits_and_errors_are_told_apart() {
        let finished = |data: &str| parser().feed(event(data).as_bytes());
        let blocked = finished(r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#).unwrap();
        assert_eq!(
            blocked.last(),
            Some(&ModelEvent::Finished(FinishReason::ContentFilter))
        );
        let cut = finished(r#"{"candidates":[{"finishReason":"MAX_TOKENS"}]}"#).unwrap();
        assert_eq!(
            cut.last(),
            Some(&ModelEvent::Finished(FinishReason::Length))
        );
        let slipped = finished(r#"{"candidates":[{"finishReason":"MALFORMED_FUNCTION_CALL"}]}"#);
        assert!(is_transient(&slipped.unwrap_err()));
        let busy = finished(r#"{"error":{"code":503,"status":"UNAVAILABLE","message":"x"}}"#);
        assert!(is_transient(&busy.unwrap_err()));
        let key = finished(r#"{"error":{"code":401,"status":"UNAUTHENTICATED","message":"x"}}"#);
        assert!(!is_transient(&key.unwrap_err()));
    }
}
