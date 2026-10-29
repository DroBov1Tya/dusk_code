//! What the screen shows of a thread: a list of entries built from the stored history and
//! then from live events. Thinking, tool calls and summaries fold into one line.

use super::paste;
use dusk_core::{
    providers::ToolCall,
    state::{Message, MessageRole},
    tools::ToolResult,
};

pub enum Entry {
    /// A sent message; `open` shows its pasted blocks in full.
    User {
        text: String,
        open: bool,
    },
    Answer(String),
    Reasoning {
        text: String,
        open: bool,
    },
    Tool {
        call: ToolCall,
        /// `None` while the tool runs.
        result: Option<ToolResult>,
        open: bool,
    },
    /// What a compaction left in place of the older messages.
    Summary {
        text: String,
        open: bool,
    },
    Notice {
        text: String,
        tone: Tone,
    },
}

#[derive(Clone, Copy)]
pub enum Tone {
    Info,
    Error,
}

#[derive(Default)]
pub struct Transcript {
    pub entries: Vec<Entry>,
}

impl Transcript {
    /// The stored messages as entries; system messages are not shown.
    pub fn from_history(messages: &[Message]) -> Self {
        let mut transcript = Self::default();
        for message in messages {
            match message.role {
                MessageRole::User => transcript.push(Entry::User {
                    text: message.content.clone(),
                    open: false,
                }),
                MessageRole::Assistant => {
                    if let Some(reasoning) = &message.reasoning {
                        transcript.add_reasoning(reasoning);
                    }
                    transcript.start_answer(&message.content);
                    for call in &message.tool_calls {
                        transcript.start_tool(call.clone());
                    }
                }
                MessageRole::Tool => {
                    // The core starts the result of a call that failed or never finished
                    // with one of these.
                    let failed = ["error:", "stopped:", "not run:", "no result:"]
                        .iter()
                        .any(|prefix| message.content.starts_with(prefix));
                    let result = ToolResult {
                        content: message.content.clone(),
                        is_error: failed,
                    };
                    let call_id = message.tool_call_id.clone().unwrap_or_default();
                    transcript.finish_tool(&call_id, result);
                }
                // System messages are instructions, not part of the conversation.
                _ => {}
            }
        }
        transcript
    }

    pub fn push(&mut self, entry: Entry) {
        self.entries.push(entry);
    }

    pub fn notice(&mut self, text: impl Into<String>, tone: Tone) {
        self.push(Entry::Notice {
            text: text.into(),
            tone,
        });
    }

    /// Streamed thinking joins the thinking block it continues.
    pub fn add_reasoning(&mut self, text: &str) {
        match self.entries.last_mut() {
            Some(Entry::Reasoning { text: last, .. }) => last.push_str(text),
            _ => self.push(Entry::Reasoning {
                text: text.to_owned(),
                open: false,
            }),
        }
    }

    pub fn add_answer(&mut self, text: &str) {
        match self.entries.last_mut() {
            Some(Entry::Answer(last)) => last.push_str(text),
            _ => self.start_answer(text),
        }
    }

    pub fn add_summary(&mut self, text: &str) {
        match self.entries.last_mut() {
            Some(Entry::Summary { text: last, .. }) => last.push_str(text),
            _ => self.push(Entry::Summary {
                text: text.to_owned(),
                open: false,
            }),
        }
    }

    pub fn start_tool(&mut self, call: ToolCall) {
        self.push(Entry::Tool {
            call,
            result: None,
            open: false,
        });
    }

    pub fn finish_tool(&mut self, call_id: &str, finished: ToolResult) {
        let running = self.entries.iter_mut().rev().find_map(|entry| match entry {
            Entry::Tool { call, result, .. } if call.id == call_id && result.is_none() => {
                Some(result)
            }
            _ => None,
        });
        if let Some(result) = running {
            *result = Some(finished);
        }
    }

    /// A turn that ended while tools ran stopped them; the core stores the same.
    pub fn stop_running_tools(&mut self) {
        for entry in &mut self.entries {
            if let Entry::Tool {
                result: result @ None,
                ..
            } = entry
            {
                *result = Some(ToolResult {
                    content: "stopped: the user interrupted the turn".into(),
                    is_error: true,
                });
            }
        }
    }

    /// What the copy button of an entry copies: its whole text, folded or not.
    pub fn text(&self, index: usize) -> Option<String> {
        Some(match self.entries.get(index)? {
            Entry::User { text, .. } => paste::plain(text),
            Entry::Answer(text) => text.clone(),
            Entry::Reasoning { text, .. } | Entry::Summary { text, .. } => text.clone(),
            Entry::Notice { text, .. } => text.clone(),
            Entry::Tool { call, result, .. } => match result {
                Some(result) => result.content.clone(),
                None => call.arguments.to_string(),
            },
        })
    }

    pub fn is_foldable(&self, index: usize) -> bool {
        match self.entries.get(index) {
            Some(Entry::User { text, .. }) => paste::has_pasted(text),
            entry => matches!(
                entry,
                Some(Entry::Reasoning { .. } | Entry::Tool { .. } | Entry::Summary { .. })
            ),
        }
    }

    pub fn toggle(&mut self, index: usize) {
        if let Some(
            Entry::User { open, .. }
            | Entry::Reasoning { open, .. }
            | Entry::Tool { open, .. }
            | Entry::Summary { open, .. },
        ) = self.entries.get_mut(index)
        {
            *open = !*open;
        }
    }

    /// Opens every thinking block, or folds them all when any is open.
    pub fn toggle_reasoning(&mut self) {
        let any_open = self
            .entries
            .iter()
            .any(|entry| matches!(entry, Entry::Reasoning { open: true, .. }));
        for entry in &mut self.entries {
            if let Entry::Reasoning { open, .. } = entry {
                *open = !any_open;
            }
        }
    }

    /// The nearest foldable entry before or after `from`.
    pub fn next_foldable(&self, from: usize, forward: bool) -> Option<usize> {
        if forward {
            (from + 1..self.entries.len()).find(|&index| self.is_foldable(index))
        } else {
            (0..from).rev().find(|&index| self.is_foldable(index))
        }
    }

    pub fn last_foldable(&self) -> Option<usize> {
        (0..self.entries.len())
            .rev()
            .find(|&index| self.is_foldable(index))
    }

    fn start_answer(&mut self, text: &str) {
        // Models often open with blank lines; a new block does not need them.
        let text = text.trim_start();
        if !text.is_empty() {
            self.push(Entry::Answer(text.to_owned()));
        }
    }
}
