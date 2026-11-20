//! The streamed message, parsed from raw bytes into model events.

use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use reqwest::Response;
use serde::Deserialize;
use serde_json::Value;

use crate::providers::{
    errors::ended_early,
    output::{FinishReason, ModelEvent, Usage},
    transport::sse::{EventParser, SseDecoder, SseStream},
};

/// Events of one streamed response, read lazily from the network.
pub type MessageStream = SseStream<MessageParser>;

pub fn stream(response: Response) -> MessageStream {
    SseStream::new(response, MessageParser::default())
}

/// Network-independent parser, so the protocol can be tested from raw bytes.
#[derive(Default)]
pub struct MessageParser {
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

impl EventParser for MessageParser {
    fn decoder(&mut self) -> &mut SseDecoder {
        &mut self.decoder
    }

    fn parse(&mut self, data: &str) -> Result<Vec<ModelEvent>> {
        self.parse_event(data)
    }

    fn done(&self) -> bool {
        self.done
    }

    /// EOF before `message_stop` is a cut stream.
    fn finish(&mut self) -> Result<()> {
        if !self.done {
            self.done = true;
            bail!(ended_early());
        }
        Ok(())
    }
}

impl MessageParser {
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
                return Err(crate::providers::stream_error(
                    error["type"].as_str().unwrap_or_default(),
                ));
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
            self.usage.cache_write_tokens = written;
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
    use crate::providers::is_transient;

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
                    ..Usage::default()
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
}
