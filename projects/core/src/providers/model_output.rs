//! What the model sends back. Every provider turns its stream into `ModelEvent`s;
//! `ModelOutput` assembles them into text, reasoning and tool calls.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::BoxFuture;

/// Bounds memory for a provider that never finishes; every kind of output counts.
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

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
            FinishReason::Length => bail!("model answer was cut by the token limit"),
            FinishReason::ContentFilter => bail!("model answer was blocked by the content filter"),
        }
        let calls = self
            .calls
            .iter()
            .map(|call| {
                // An empty string is how some servers send "no arguments".
                let arguments = match call.arguments.trim() {
                    "" => Value::Object(Default::default()),
                    json => serde_json::from_str(json)
                        .with_context(|| format!("invalid arguments for tool {}", call.name))?,
                };
                Ok(ToolCall {
                    id: call.id.clone(),
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
    fn keeps_reasoning_apart_from_text() {
        let (output, calls) = assemble(&[
            ModelEvent::Reasoning("think".into()),
            ModelEvent::Text("Hel".into()),
            ModelEvent::Text("lo".into()),
            ModelEvent::Finished(FinishReason::Stop),
        ])
        .unwrap();
        assert_eq!(
            (output.text.as_str(), output.reasoning.as_str()),
            ("Hello", "think")
        );
        assert!(calls.is_empty());
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
    fn empty_arguments_mean_no_arguments() {
        let (_, calls) = assemble(&[
            call(0, "a", "now"),
            ModelEvent::Finished(FinishReason::ToolCalls),
        ])
        .unwrap();
        assert_eq!(calls[0].arguments, json!({}));
    }

    #[test]
    fn rejects_broken_responses() {
        let text = || ModelEvent::Text("x".into());
        let stop = || ModelEvent::Finished(FinishReason::Stop);
        let cases = [
            ("no finish reason", vec![text()]),
            ("empty answer", vec![stop()]),
            (
                "cut by length",
                vec![text(), ModelEvent::Finished(FinishReason::Length)],
            ),
            ("index gap", vec![call(1, "a", "x"), stop()]),
            ("unknown call", vec![arguments(0, "{}"), stop()]),
            (
                "bad JSON",
                vec![call(0, "a", "x"), arguments(0, "{"), stop()],
            ),
        ];
        for (case, events) in cases {
            assert!(assemble(&events).is_err(), "{case}");
        }
    }

    #[test]
    fn rejects_oversized_output() {
        let mut output = ModelOutput::default();
        let half = "a".repeat(MAX_OUTPUT_BYTES / 2 + 1);
        output.apply(&ModelEvent::Reasoning(half.clone())).unwrap();
        assert!(output.apply(&ModelEvent::Text(half)).is_err());
    }
}
