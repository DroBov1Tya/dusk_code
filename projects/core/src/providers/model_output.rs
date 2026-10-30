//! What the model sends back. Every provider turns its stream into `ModelEvent`s;
//! `ModelOutput` assembles them into text, reasoning and tool calls.

use std::fmt;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::BoxFuture;

/// Bounds memory for a provider that never finishes; every kind of output counts.
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
/// Tool calls one answer may make; real answers make a few.
const MAX_TOOL_CALLS: usize = 128;

/// One increment of a streamed model response.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelEvent {
    /// Model thinking: shown to the user and never sent back as context.
    Reasoning(String),
    Text(String),
    /// A new tool call; its arguments follow as `ToolArguments` with the same index.
    ToolCall {
        index: u32,
        id: String,
        name: String,
    },
    /// A piece of the JSON arguments of the call with this index.
    ToolArguments {
        index: u32,
        fragment: String,
    },
    Usage(Usage),
    Finished(FinishReason),
}

/// Token usage of the whole request, as the provider reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// Everything the request sent, the cached part included.
    pub prompt_tokens: u64,
    /// The part of the prompt the provider served from its cache, usually at a discount.
    pub cached_prompt_tokens: u64,
    /// Everything the model wrote, its reasoning included.
    pub completion_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FinishReason {
    Stop,
    ToolCalls,
    /// Cut by the output token limit.
    Length,
    ContentFilter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned; a tool result refers back to it.
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// The model wrote as much as it may: the provider cut it at `max_tokens`, or the core
/// stopped the stream at its own count.
#[derive(Debug)]
pub struct OutputLimit;

impl fmt::Display for OutputLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the answer reached the output token limit")
    }
}

impl std::error::Error for OutputLimit {}

pub fn is_output_limit(error: &anyhow::Error) -> bool {
    error.downcast_ref::<OutputLimit>().is_some()
}

/// A streamed model response.
pub trait ChatStream: Send {
    /// Returns `None` once the response has ended; an error ends the stream.
    fn next_event(&mut self) -> BoxFuture<'_, Option<ModelEvent>>;
}

/// The response assembled so far. It stays usable when the stream stops early.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ModelOutput {
    pub text: String,
    pub reasoning: String,
    pub usage: Option<Usage>,
    calls: Vec<PendingCall>,
    finish: Option<FinishReason>,
    size: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct PendingCall {
    id: String,
    name: String,
    arguments: String,
}

impl ModelOutput {
    /// Estimated size of everything written so far, at four bytes per token.
    pub fn estimated_tokens(&self) -> u64 {
        self.size.div_ceil(4) as u64
    }

    /// Nothing the user could have seen yet: no text, reasoning or tool call.
    pub fn is_empty(&self) -> bool {
        self.size == 0 && self.calls.is_empty()
    }

    pub fn apply(&mut self, event: &ModelEvent) -> Result<()> {
        match event {
            ModelEvent::Reasoning(text) => self.grow(text, |output| &mut output.reasoning)?,
            ModelEvent::Text(text) => self.grow(text, |output| &mut output.text)?,
            ModelEvent::ToolCall { index, id, name } => {
                ensure!(
                    *index as usize == self.calls.len(),
                    "tool calls arrived out of order"
                );
                ensure!(!name.trim().is_empty(), "tool call has no name");
                ensure!(
                    self.calls.len() < MAX_TOOL_CALLS,
                    "model made more than {MAX_TOOL_CALLS} tool calls in one answer"
                );
                self.size += id.len() + name.len();
                ensure!(self.size <= MAX_OUTPUT_BYTES, "model answer is too large");
                self.calls.push(PendingCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: String::new(),
                });
            }
            ModelEvent::ToolArguments { index, fragment } => {
                let index = *index as usize;
                ensure!(
                    index < self.calls.len(),
                    "arguments for an unknown tool call"
                );
                self.grow(fragment, |output| &mut output.calls[index].arguments)?;
            }
            ModelEvent::Usage(usage) => self.usage = Some(*usage),
            ModelEvent::Finished(reason) => self.finish = Some(*reason),
        }
        Ok(())
    }

    /// Checks that the model finished normally and parses the tool call arguments.
    pub fn finish(&self) -> Result<Vec<ToolCall>> {
        match self.finish.context("model response has no finish reason")? {
            FinishReason::Stop | FinishReason::ToolCalls => {}
            FinishReason::Length => return Err(OutputLimit.into()),
            FinishReason::ContentFilter => bail!("model answer was blocked by the content filter"),
        }
        let calls = self
            .calls
            .iter()
            .enumerate()
            .map(|(index, call)| {
                // An empty string is how some servers send "no arguments". Text that is not
                // JSON is kept as it came: the tool call fails, and the model is told why.
                let arguments = match call.arguments.trim() {
                    "" => Value::Object(Default::default()),
                    json => serde_json::from_str(json)
                        .unwrap_or_else(|_| Value::String(call.arguments.clone())),
                };
                // Results are matched to calls by ID, so a missing or repeated one is
                // replaced; some servers send the same ID for parallel calls.
                let repeated = self.calls[..index].iter().any(|other| other.id == call.id);
                let id = if call.id.trim().is_empty() || repeated {
                    format!("call_{index}")
                } else {
                    call.id.clone()
                };
                Ok(ToolCall {
                    id,
                    name: call.name.clone(),
                    arguments,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            !self.text.trim().is_empty() || !calls.is_empty(),
            "model returned an empty answer"
        );
        Ok(calls)
    }

    fn grow(&mut self, text: &str, field: impl FnOnce(&mut Self) -> &mut String) -> Result<()> {
        self.size += text.len();
        ensure!(self.size <= MAX_OUTPUT_BYTES, "model answer is too large");
        field(self).push_str(text);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn assemble(events: &[ModelEvent]) -> Result<(ModelOutput, Vec<ToolCall>)> {
        let mut output = ModelOutput::default();
        for event in events {
            output.apply(event)?;
        }
        let calls = output.finish()?;
        Ok((output, calls))
    }

    fn call(index: u32, id: &str, name: &str) -> ModelEvent {
        ModelEvent::ToolCall {
            index,
            id: id.into(),
            name: name.into(),
        }
    }

    fn arguments(index: u32, fragment: &str) -> ModelEvent {
        ModelEvent::ToolArguments {
            index,
            fragment: fragment.into(),
        }
    }

    #[test]
    fn assembles_interleaved_tool_call_arguments() {
        let (_, calls) = assemble(&[
            call(0, "a", "weather"),
            arguments(0, "{\"city\":"),
            call(1, "b", "weather"),
            arguments(1, "{\"city\": \"Kazan\"}"),
            arguments(0, " \"Moscow\"}"),
            ModelEvent::Finished(FinishReason::ToolCalls),
        ])
        .unwrap();
        assert_eq!(calls[0].arguments, json!({"city": "Moscow"}));
        assert_eq!(calls[1].id, "b");
        assert_eq!(calls[1].arguments, json!({"city": "Kazan"}));
    }

    #[test]
    fn missing_and_repeated_call_ids_are_made_unique() {
        let (_, calls) = assemble(&[
            call(0, "same", "weather"),
            call(1, "same", "weather"),
            call(2, "", "weather"),
            ModelEvent::Finished(FinishReason::ToolCalls),
        ])
        .unwrap();
        let ids: Vec<&str> = calls.iter().map(|call| call.id.as_str()).collect();
        assert_eq!(ids, ["same", "call_1", "call_2"]);
    }

    #[test]
    fn arguments_that_are_not_json_stay_as_written() {
        let (_, calls) = assemble(&[
            call(0, "a", "read_file"),
            arguments(0, "{\"path\": \"cut"),
            ModelEvent::Finished(FinishReason::ToolCalls),
        ])
        .unwrap();
        assert_eq!(calls[0].arguments, json!("{\"path\": \"cut"));
    }

    #[test]
    fn rejects_oversized_output() {
        let mut output = ModelOutput::default();
        let half = "a".repeat(MAX_OUTPUT_BYTES / 2 + 1);
        output.apply(&ModelEvent::Reasoning(half.clone())).unwrap();
        assert!(output.apply(&ModelEvent::Text(half)).is_err());
    }
}
