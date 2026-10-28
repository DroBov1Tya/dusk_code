//! Fallback for servers that leave reasoning and tool calls inside the answer text:
//! `<think>...</think>` and Hermes-style `<tool_call>{"name": .., "arguments": ..}</tool_call>`
//! become the same `ModelEvent`s a native provider sends.
//!
//! Text passes through at once; only a trailing piece that may be the start of a tag,
//! split between two chunks, is held back until the next chunk decides.

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::Value;

use super::model_output::ModelEvent;

const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";
const CALL_OPEN: &str = "<tool_call>";
const CALL_CLOSE: &str = "</tool_call>";
const MAX_CALL_BYTES: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub struct TagSplitter {
    state: State,
    pending: String,
    calls: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Text,
    Think,
    ToolCall,
}

#[derive(Deserialize)]
struct InlineCall {
    name: String,
    #[serde(default)]
    arguments: Value,
}

impl TagSplitter {
    pub fn feed(&mut self, text: &str) -> Result<Vec<ModelEvent>> {
        self.pending.push_str(text);
        let mut events = Vec::new();
        while self.step(&mut events)? {}
        Ok(events)
    }

    /// Flushes what is left when the stream ends. An unclosed tool call is an error.
    pub fn finish(&mut self) -> Result<Vec<ModelEvent>> {
        ensure!(
            self.state != State::ToolCall,
            "unterminated inline tool call"
        );
        let rest = std::mem::take(&mut self.pending);
        Ok(self.text_event(rest).into_iter().collect())
    }

    /// Handles the next tag or emits safe text; returns whether to look again.
    fn step(&mut self, events: &mut Vec<ModelEvent>) -> Result<bool> {
        match self.state {
            State::Text => {
                let next = [(THINK_OPEN, State::Think), (CALL_OPEN, State::ToolCall)]
                    .into_iter()
                    .filter_map(|(tag, state)| Some((self.pending.find(tag)?, tag, state)))
                    .min_by_key(|(at, _, _)| *at);
                match next {
                    Some((at, tag, state)) => {
                        let before = self.take(at, tag.len());
                        events.extend(self.text_event(before));
                        self.state = state;
                        Ok(true)
                    }
                    None => {
                        let safe = self.safe_len(&[THINK_OPEN, CALL_OPEN]);
                        let text = self.take(safe, 0);
                        events.extend(self.text_event(text));
                        Ok(false)
                    }
                }
            }
            State::Think => match self.pending.find(THINK_CLOSE) {
                Some(at) => {
                    let reasoning = self.take(at, THINK_CLOSE.len());
                    events.extend(self.text_event(reasoning));
                    self.state = State::Text;
                    Ok(true)
                }
                None => {
                    let safe = self.safe_len(&[THINK_CLOSE]);
                    let reasoning = self.take(safe, 0);
                    events.extend(self.text_event(reasoning));
                    Ok(false)
                }
            },
            State::ToolCall => match self.pending.find(CALL_CLOSE) {
                Some(at) => {
                    let body = self.take(at, CALL_CLOSE.len());
                    events.extend(self.call_events(&body)?);
                    self.state = State::Text;
                    Ok(true)
                }
                None => {
                    ensure!(
                        self.pending.len() <= MAX_CALL_BYTES,
                        "inline tool call is too large"
                    );
                    Ok(false)
                }
            },
        }
    }

    /// Removes and returns the first `len` bytes, then drops `skip` more (a tag).
    fn take(&mut self, len: usize, skip: usize) -> String {
        let rest = self.pending.split_off(len + skip);
        let mut taken = std::mem::replace(&mut self.pending, rest);
        taken.truncate(len);
        taken
    }

    /// Length of the pending text that cannot be the start of any of `tags`.
    fn safe_len(&self, tags: &[&str]) -> usize {
        let held = tags
            .iter()
            .flat_map(|tag| (1..tag.len()).map(move |n| &tag[..n]))
            .filter(|prefix| self.pending.ends_with(prefix))
            .map(str::len)
            .max()
            .unwrap_or(0);
        self.pending.len() - held
    }

    /// Text in the current state: answer text outside tags, reasoning inside `<think>`.
    fn text_event(&self, text: String) -> Option<ModelEvent> {
        if text.is_empty() {
            return None;
        }
        Some(match self.state {
            State::Think => ModelEvent::Reasoning(text),
            State::Text | State::ToolCall => ModelEvent::Text(text),
        })
    }

    fn call_events(&mut self, body: &str) -> Result<[ModelEvent; 2]> {
        let call: InlineCall =
            serde_json::from_str(body.trim()).context("invalid inline tool call JSON")?;
        let index = self.calls;
        self.calls += 1;
        Ok([
            ModelEvent::ToolCall {
                index,
                id: format!("call_{index}"),
                name: call.name,
            },
            ModelEvent::ToolArguments {
                index,
                fragment: call.arguments.to_string(),
            },
        ])
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn split(chunks: &[&str]) -> Result<Vec<ModelEvent>> {
        let mut splitter = TagSplitter::default();
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(splitter.feed(chunk)?);
        }
        events.extend(splitter.finish()?);
        Ok(merge(events))
    }

    /// Joins neighbouring text pieces of the same kind, so chunking does not matter.
    fn merge(events: Vec<ModelEvent>) -> Vec<ModelEvent> {
        let mut merged: Vec<ModelEvent> = Vec::new();
        for event in events {
            match (merged.last_mut(), event) {
                (Some(ModelEvent::Text(last)), ModelEvent::Text(text))
                | (Some(ModelEvent::Reasoning(last)), ModelEvent::Reasoning(text)) => {
                    last.push_str(&text)
                }
                (_, event) => merged.push(event),
            }
        }
        merged
    }

    #[test]
    fn splits_thinking_text_and_tool_calls() {
        let events = split(&[
            "<think>plan</think>Sure. <tool_call>{\"name\": \"weather\", \"arguments\": {\"city\": \"Kazan\"}}</tool_call>",
        ])
        .unwrap();
        assert_eq!(
            events,
            [
                ModelEvent::Reasoning("plan".into()),
                ModelEvent::Text("Sure. ".into()),
                ModelEvent::ToolCall {
                    index: 0,
                    id: "call_0".into(),
                    name: "weather".into()
                },
                ModelEvent::ToolArguments {
                    index: 0,
                    fragment: "{\"city\":\"Kazan\"}".into()
                },
            ]
        );
    }

    #[test]
    fn holds_back_only_a_possible_tag_start() {
        let mut splitter = TagSplitter::default();
        assert_eq!(
            splitter.feed("a < b <thi").unwrap(),
            [ModelEvent::Text("a < b ".into())]
        );
        assert_eq!(
            splitter.feed("s is text").unwrap(),
            [ModelEvent::Text("<this is text".into())]
        );
    }

    #[test]
    fn rejects_broken_tool_calls() {
        assert!(
            split(&["<tool_call>{\"name\": \"x\""]).is_err(),
            "unterminated"
        );
        assert!(split(&["<tool_call>not json</tool_call>"]).is_err());
    }

    proptest! {
        #[test]
        fn chunking_does_not_change_events(
            cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..12),
        ) {
            let text = "Hi <think>step 1\nstep 2</think>ok <tool_call>{\"name\":\"t\",\"arguments\":{\"q\":\"<x>\"}}</tool_call> bye <th";
            let mut points: Vec<usize> = cuts.iter().map(|cut| cut.index(text.len() + 1)).collect();
            points.sort_unstable();
            let mut chunks = Vec::new();
            let mut start = 0;
            for point in points {
                chunks.push(&text[start..point]);
                start = point;
            }
            chunks.push(&text[start..]);
            prop_assert_eq!(split(&chunks).unwrap(), split(&[text]).unwrap());
        }

        #[test]
        fn arbitrary_text_never_panics(chunks in prop::collection::vec(".{0,20}", 0..10)) {
            let mut splitter = TagSplitter::default();
            for chunk in &chunks {
                if splitter.feed(chunk).is_err() {
                    return Ok(());
                }
            }
            let _ = splitter.finish();
        }
    }
}
