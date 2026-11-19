use crate::{model::ToolCall, tools::ToolResult};

/// How much of a tool's arguments and answer an unfolded step shows.
pub const SHOWN_CHARS: usize = 4000;

/// One tool call as a chat shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    pub call_id: String,
    pub name: String,
    /// The first line of what it was asked: its command, path or query.
    pub detail: String,
    /// The arguments, clipped, for the unfolded step.
    pub arguments: String,
    /// The start of what the tool answered, once it has.
    pub output: String,
    pub state: ToolState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Running,
    Done,
    Failed,
}

/// How the core starts the stored result of a call that failed, was stopped, was never
/// run or never finished. A stored tool message has no error flag; these say it.
pub const FAILED_PREFIXES: [&str; 4] = ["error:", "stopped:", "not run:", "no result:"];

/// Whether a stored tool result says its call failed or did not finish.
pub fn stored_failure(content: &str) -> bool {
    FAILED_PREFIXES
        .iter()
        .any(|prefix| content.starts_with(prefix))
}

impl Tool {
    /// A call as it starts live, or `Done` for one read from history.
    pub fn new(call: &ToolCall, state: ToolState) -> Tool {
        Tool {
            call_id: call.id.clone(),
            name: call.name.clone(),
            detail: tool_detail(&call.arguments),
            arguments: clip(&call.arguments.to_string()),
            output: String::new(),
            state,
        }
    }

    pub fn finish(&mut self, result: &ToolResult) {
        self.state = match result.is_error {
            true => ToolState::Failed,
            false => ToolState::Done,
        };
        self.output = clip(&result.content);
    }

    /// The result as the history stored it: failed when it starts with one of
    /// `FAILED_PREFIXES`, done otherwise.
    pub fn finish_stored(&mut self, content: &str) {
        self.state = match stored_failure(content) {
            true => ToolState::Failed,
            false => ToolState::Done,
        };
        self.output = clip(content);
    }
}

/// The first `SHOWN_CHARS` characters, marked when more were cut.
pub fn clip(text: &str) -> String {
    let mut clipped: String = text.chars().take(SHOWN_CHARS).collect();
    if clipped.len() < text.len() {
        clipped.push_str("\n...");
    }
    clipped
}

/// The arguments that say what a tool was asked, in the order they are looked for.
pub const DETAIL_KEYS: [&str; 7] = [
    "description",
    "command",
    "path",
    "query",
    "url",
    "pattern",
    "task",
];

/// A line of what a tool was asked: the first of `DETAIL_KEYS` that holds text. A skill,
/// loaded by its name, shows that name where no other argument says what was asked.
pub fn tool_detail(arguments: &serde_json::Value) -> String {
    let text = DETAIL_KEYS
        .iter()
        .chain(&["name"])
        .find_map(|key| arguments.get(key).and_then(|value| value.as_str()))
        .unwrap_or_default();
    let line = text.lines().next().unwrap_or_default();
    line.chars().take(160).collect()
}
