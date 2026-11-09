//! The streamed completion, parsed from raw bytes into model events.

use std::collections::VecDeque;

use anyhow::{Result, anyhow, bail};
use reqwest::Response;
use serde::Deserialize;
use serde_json::Value;

use crate::providers::{
    errors::{Transient, UnsupportedEffort},
    output::{FinishReason, ModelEvent, Usage},
    transport::sse::SseDecoder,
};

use super::client::error_rejects_effort_off;

const DONE_MARKER: &str = "[DONE]";

pub struct CompletionStream {
    response: Response,
    parser: CompletionParser,
    pending: VecDeque<ModelEvent>,
    finished: bool,
}

impl CompletionStream {
    pub fn new(response: Response, effort_off: bool) -> Self {
        Self {
            response,
            parser: CompletionParser {
                effort_off,
                ..CompletionParser::default()
            },
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
#[derive(Default)]
struct CompletionParser {
    decoder: SseDecoder,
    effort_off: bool,
    finish_seen: bool,
    done: bool,
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<WireUsage>,
    error: Option<Value>,
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
    prompt_tokens_details: Option<PromptDetails>,
    completion_tokens_details: Option<CompletionDetails>,
    /// DeepSeek reports cache hits here, and may leave out the details above.
    prompt_cache_hit_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct PromptDetails {
    cached_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct CompletionDetails {
    reasoning_tokens: Option<u64>,
}

impl CompletionParser {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<ModelEvent>> {
        let mut events = Vec::new();
        for data in self.decoder.feed(bytes)? {
            if self.done {
                break;
            }
            if data == DONE_MARKER {
                // Some servers end with [DONE] alone; it still means the answer is whole.
                if !self.finish_seen {
                    self.finish_seen = true;
                    events.push(ModelEvent::Finished(FinishReason::Stop));
                }
                self.finish()?;
                break;
            }
            events.extend(self.parse_chunk(&data)?);
        }
        Ok(events)
    }

    /// The model is done once it reported a finish reason or `[DONE]`; EOF before either is
    /// a cut stream.
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
        if let Some(error) = chunk.error {
            if self.effort_off && error_rejects_effort_off(&error) {
                bail!(UnsupportedEffort);
            }
            // Type and code only: the message can echo the input.
            let kind = format!("{} {}", error["type"], error["code"]);
            return Err(crate::providers::stream_error(&kind));
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
                cached_prompt_tokens: usage
                    .prompt_tokens_details
                    .and_then(|details| details.cached_tokens)
                    .or(usage.prompt_cache_hit_tokens)
                    .unwrap_or(0),
                completion_tokens: usage.completion_tokens,
                reasoning_tokens: usage
                    .completion_tokens_details
                    .and_then(|details| details.reasoning_tokens)
                    .unwrap_or(0),
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

    fn chunk(delta: &str) -> String {
        format!("data: {{\"choices\":[{{\"index\":0,\"delta\":{delta}}}]}}\n\n")
    }

    fn finish(reason: &str) -> String {
        format!(
            "data: {{\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"{reason}\"}}]}}\n\n"
        )
    }

    #[test]
    fn deepseek_cache_hits_count_as_cached() {
        let body = "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":1,\"prompt_cache_hit_tokens\":8,\"prompt_cache_miss_tokens\":2}}\n\n";
        let events = CompletionParser::default().feed(body.as_bytes()).unwrap();
        assert!(matches!(
            events.as_slice(),
            [ModelEvent::Usage(Usage {
                cached_prompt_tokens: 8,
                ..
            })]
        ));
    }

    #[test]
    fn emits_reasoning_text_finish_and_usage_in_order() {
        let body = [
            chunk(r#"{"role":"assistant","content":""}"#),
            chunk(r#"{"reasoning_content":"think"}"#),
            chunk(r#"{"content":"Hi"}"#),
            finish("stop"),
            concat!(
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,",
                "\"prompt_tokens_details\":{\"cached_tokens\":3},",
                "\"completion_tokens_details\":{\"reasoning_tokens\":1}}}\n\n"
            )
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
                    cached_prompt_tokens: 3,
                    completion_tokens: 2,
                    reasoning_tokens: 1,
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
    fn a_cut_stream_may_pass_later() {
        let cut = CompletionParser::default().finish().unwrap_err();
        assert!(crate::providers::is_transient(&cut));
    }

    #[test]
    fn a_stream_ends_with_a_finish_reason_or_done_and_not_with_eof() {
        let mut parser = CompletionParser::default();
        parser
            .feed(chunk(r#"{"content":"partial"}"#).as_bytes())
            .unwrap();
        assert!(parser.finish().is_err(), "EOF before the end is a cut");

        let mut parser = CompletionParser::default();
        let body = format!("{}data: [DONE]\n\n", chunk(r#"{"content":"whole"}"#));
        let events = parser.feed(body.as_bytes()).unwrap();
        assert_eq!(
            events.last(),
            Some(&ModelEvent::Finished(FinishReason::Stop))
        );
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
