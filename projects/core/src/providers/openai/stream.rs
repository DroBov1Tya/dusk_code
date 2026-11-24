//! The streamed completion, parsed from raw bytes into model events.

use anyhow::{Result, anyhow, bail};
use reqwest::Response;
use serde::Deserialize;
use serde_json::Value;

use crate::providers::{
    cost,
    errors::{RefusedEffort, UnsupportedEffort, ended_early},
    output::{FinishReason, ModelEvent, Usage},
    transport::sse::{EventParser, SseDecoder, SseStream},
};

use super::client::error_rejects_effort;
use crate::config::Effort;

const DONE_MARKER: &str = "[DONE]";

pub type CompletionStream = SseStream<CompletionParser>;

pub fn stream(response: Response, effort: Option<Effort>) -> CompletionStream {
    let parser = CompletionParser {
        effort,
        ..CompletionParser::default()
    };
    SseStream::new(response, parser)
}

/// Network-independent parser, so the protocol can be tested from raw bytes.
#[derive(Default)]
pub struct CompletionParser {
    decoder: SseDecoder,
    /// The level the request carried, to tell a refusal of it from another error.
    effort: Option<Effort>,
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
    /// OpenRouter's cost of the call in dollars.
    cost: Option<f64>,
    /// xAI's cost of the call in ten-billionths of a dollar.
    cost_in_usd_ticks: Option<u64>,
}

#[derive(Deserialize)]
struct PromptDetails {
    cached_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct CompletionDetails {
    reasoning_tokens: Option<u64>,
}

impl EventParser for CompletionParser {
    fn decoder(&mut self) -> &mut SseDecoder {
        &mut self.decoder
    }

    fn parse(&mut self, data: &str) -> Result<Vec<ModelEvent>> {
        if data == DONE_MARKER {
            // Some servers end with [DONE] alone; it still means the answer is whole.
            let mut events = Vec::new();
            if !self.finish_seen {
                self.finish_seen = true;
                events.push(ModelEvent::Finished(FinishReason::Stop));
            }
            self.finish()?;
            return Ok(events);
        }
        self.parse_chunk(data)
    }

    fn done(&self) -> bool {
        self.done
    }

    /// The model is done once it reported a finish reason or `[DONE]`; EOF before either is
    /// a cut stream.
    fn finish(&mut self) -> Result<()> {
        self.done = true;
        if !self.finish_seen {
            bail!(ended_early());
        }
        Ok(())
    }
}

impl CompletionParser {
    fn parse_chunk(&mut self, data: &str) -> Result<Vec<ModelEvent>> {
        // Upstream payloads can echo input; never copy them into errors.
        let chunk: Chunk = serde_json::from_str(data)
            .map_err(|_| anyhow!("invalid provider stream chunk JSON"))?;
        if let Some(error) = chunk.error {
            let refused = self.effort.is_some() && error_rejects_effort(&error);
            if refused && self.effort == Some(Effort::Off) {
                bail!(UnsupportedEffort);
            }
            // Type and code only: the message can echo the input.
            let kind = format!("{} {}", error["type"], error["code"]);
            let failure = crate::providers::stream_error(&kind);
            if refused {
                bail!(RefusedEffort(failure));
            }
            return Err(failure);
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
                cache_write_tokens: 0,
                cost: usage
                    .cost_in_usd_ticks
                    .map(cost::reported_ticks)
                    .or_else(|| usage.cost.and_then(cost::reported_usd)),
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
    fn a_refused_level_of_effort_is_told_apart_from_off_and_other_errors() {
        use crate::providers::{is_refused_effort, is_unsupported_effort, provider_error};
        let refused = concat!(
            "data: {\"error\":{\"type\":\"invalid_request_error\",",
            "\"code\":\"unsupported_parameter\",\"param\":\"reasoning_effort\",",
            "\"message\":\"reasoning_effort is not supported with this model\"}}\n\n"
        );
        let feed = |effort| {
            CompletionParser {
                effort,
                ..CompletionParser::default()
            }
            .feed(refused.as_bytes())
            .unwrap_err()
        };
        // A level the model does not take: the provider's failure, marked as refused.
        let high = feed(Some(Effort::High));
        assert!(is_refused_effort(&high));
        assert!(provider_error(&high).is_some());
        // Off keeps its own path, which restores the user's setting.
        let off = feed(Some(Effort::Off));
        assert!(is_unsupported_effort(&off) && !is_refused_effort(&off));
        // Without an effort sent, the same text is an ordinary failure.
        assert!(!is_refused_effort(&feed(None)));
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
    fn a_reported_cost_comes_with_the_usage() {
        let body = "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":1,\"cost\":0.0042}}\n\n";
        let events = CompletionParser::default().feed(body.as_bytes()).unwrap();
        let [ModelEvent::Usage(usage)] = events.as_slice() else {
            panic!("one usage event");
        };
        assert_eq!(usage.cost.map(|cost| cost.micro_usd), Some(4_200));
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
                    ..Usage::default()
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
