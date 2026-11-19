//! What the screen shows of a thread: a list of entries built from the stored history and
//! then from live events. Thinking, tool calls and summaries fold into one line.

use ratatui::{
    layout::{Rect, Size},
    text::Line,
};
use std::{
    collections::HashSet,
    sync::atomic::{AtomicU64, Ordering},
};

use super::paste;
use dusk_protocol::{
    catalog::{SkillChange, SkillSaveAction},
    model::ToolCall,
    thread::{Block, JobResult, Message, MessageRole},
    tools::{Answered, Asked, Card, ToolResult},
};

pub enum Entry {
    /// A sent message; `open` shows its pasted blocks in full.
    User {
        text: String,
        open: bool,
        /// The agent's questions this message answers, drawn in place of its text.
        answered: Vec<Answered>,
    },
    /// What a background job left when it ended; `open` shows its output.
    Job {
        result: JobResult,
        open: bool,
    },
    Answer(String),
    /// Stands before a run of thinking and tool calls: one line that says what goes on,
    /// with the run under it once opened.
    Activity {
        open: bool,
    },
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
        local_id: Option<String>,
        text: String,
        tone: Tone,
    },
    /// One change an agent made to a skill; a change seen again replaces its line.
    Skill {
        id: i64,
        text: String,
        tone: Tone,
    },
    /// A message another agent sent this thread, from its thread `thread_id`.
    FromAgent {
        thread_id: String,
        text: String,
        open: bool,
    },
    /// What the agent showed with the `show` tool, in place of that tool step.
    Block(Block),
}

/// Thinking and tool calls: the steps an activity line stands for.
fn is_step(entry: &Entry) -> bool {
    matches!(entry, Entry::Reasoning { .. } | Entry::Tool { .. })
}

/// Longer agent messages start folded.
const OPEN_AGENT_LINES: usize = 12;

impl Entry {
    pub fn from_agent(thread_id: String, text: String) -> Self {
        let open = text.lines().count() <= OPEN_AGENT_LINES;
        Self::FromAgent {
            thread_id,
            text,
            open,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Tone {
    Info,
    Error,
    Success,
}

#[derive(Default)]
pub struct Transcript {
    pub entries: Vec<Entry>,
    /// When each entry came, in ms since the epoch, at the same index.
    times: Vec<i64>,
    /// The time of the stored message being read in; live entries take the clock's.
    stamp: Option<i64>,
    /// Entries the user hid; each shows as one line until shown again.
    pub hidden: HashSet<usize>,
    /// What the agent does now, for the activity line of the run in progress.
    live: Option<String>,
    /// Only the root transcript is connected to the local image store.
    pub images: super::images::Images,
    /// What the agent asked and the user has not answered yet.
    asked: Asked,
    /// Changes when anything but the text of the last entry changes, so the lines of the
    /// earlier entries can be kept between frames. Unique across transcripts.
    revision: u64,
}

/// The drawn lines of every entry but the last, kept until the transcript, the width
/// or the browsed entry changes, so a frame renders only the entry still being written.
pub struct Layout {
    pub width: usize,
    pub revision: u64,
    pub browsed: Option<usize>,
    pub lines: Vec<Line<'static>>,
    /// Each line as plain text, for copying a selection.
    pub text: Vec<String>,
    /// The line each entry starts on.
    pub starts: Vec<usize>,
    pub images: Vec<ImageSpot>,
}

/// The rows of an image preview on screen: where they are, the card's whole size in
/// cells, and how many of its rows are scrolled off above.
pub struct ShownPreview {
    pub area: Rect,
    pub card: Size,
    pub skip: u16,
    pub artifact: Card,
}

/// An image preview in the lines: its first line, its column and size in cells.
#[derive(Clone)]
pub struct ImageSpot {
    pub line: usize,
    pub column: u16,
    pub width: u16,
    pub height: u16,
    pub artifact: Card,
}

/// Revisions come from one counter, so a new transcript never matches an old one.
fn next_revision() -> u64 {
    static REVISIONS: AtomicU64 = AtomicU64::new(1);
    REVISIONS.fetch_add(1, Ordering::Relaxed)
}

/// What an opened job result shows on screen: this many lines, and no more than this many characters.
const JOB_LINES: usize = 12;
const JOB_CHARS: usize = 1200;

impl Transcript {
    pub fn set_artifact_store(&mut self, store: dusk_core::app::ArtifactStore) {
        self.images = super::images::Images::new(store);
        self.changed();
    }

    pub fn set_live(&mut self, live: Option<String>) {
        if self.live != live {
            self.live = live;
            self.changed();
        }
    }

    /// The run under the activity line at `index`: its steps.
    pub fn run(&self, index: usize) -> impl Iterator<Item = &Entry> {
        self.entries[index + 1..]
            .iter()
            .take_while(|entry| is_step(entry))
    }

    /// What the activity line at `index` says goes on, while its run is the newest thing
    /// in the thread: the step reported, else the tool still running.
    pub fn current_step(&self, index: usize) -> Option<String> {
        if index + 1 + self.run(index).count() < self.entries.len() {
            return None;
        }
        self.live.clone().or_else(|| {
            self.run(index).find_map(|entry| match entry {
                Entry::Tool {
                    call, result: None, ..
                } => Some(call.name.clone()),
                _ => None,
            })
        })
    }

    /// A step under a folded activity line, which shows nothing of it.
    pub fn is_folded_away(&self, index: usize) -> bool {
        if !self.entries.get(index).is_some_and(is_step) {
            return false;
        }
        let line = self.entries[..index]
            .iter()
            .rev()
            .find(|entry| !is_step(entry));
        matches!(line, Some(Entry::Activity { open: false }))
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn changed(&mut self) {
        self.revision = next_revision();
    }

    /// For a change the methods here do not cover; the lines are laid out again.
    pub fn entries_mut(&mut self) -> &mut Vec<Entry> {
        self.changed();
        &mut self.entries
    }

    /// Drops an entry; the hidden ones after it move up with it.
    pub fn remove(&mut self, index: usize) {
        if index >= self.entries.len() {
            return;
        }
        self.entries.remove(index);
        if index < self.times.len() {
            self.times.remove(index);
        }
        self.hidden = self
            .hidden
            .iter()
            .filter(|&&hidden| hidden != index)
            .map(|&hidden| if hidden > index { hidden - 1 } else { hidden })
            .collect();
        self.changed();
    }

    /// The stored messages as entries; system messages are not shown.
    pub fn from_history(messages: &[Message]) -> Self {
        let mut transcript = Self::default();
        for message in messages {
            transcript.stamp = Some(message.created_at);
            match message.role {
                MessageRole::User => match &message.from_thread_id {
                    Some(from) => {
                        transcript.push(Entry::from_agent(from.clone(), message.content.clone()))
                    }
                    None => transcript.add_user(&message.content),
                },
                MessageRole::Job => transcript.job_result(&message.content),
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
                        exit_code: None,
                        artifacts: message.artifacts.clone(),
                    };
                    let call_id = message.tool_call_id.clone().unwrap_or_default();
                    transcript.finish_tool(&call_id, result);
                }
                // System messages are instructions, not part of the conversation.
                _ => {}
            }
        }
        transcript.stamp = None;
        transcript
    }

    /// When the entry at `index` came, in ms since the epoch.
    pub fn time(&self, index: usize) -> Option<i64> {
        self.times.get(index).copied()
    }

    pub fn push(&mut self, entry: Entry) {
        let in_run = matches!(
            self.entries.last(),
            Some(Entry::Activity { .. } | Entry::Reasoning { .. } | Entry::Tool { .. })
        );
        if is_step(&entry) && !in_run {
            self.entries.push(Entry::Activity { open: false });
        }
        self.entries.push(entry);
        // Also drops the times of entries popped through `entries_mut`.
        let at = self.stamp.unwrap_or_else(dusk_protocol::system::now_ms);
        self.times.resize(self.entries.len(), at);
        self.changed();
    }

    /// The user's own message; one that answers the agent's question keeps the
    /// question beside it.
    pub fn add_user(&mut self, text: &str) {
        let answered = self.asked.answer(text).unwrap_or_default();
        self.push(Entry::User {
            text: text.to_owned(),
            open: false,
            answered,
        });
    }

    /// A background job's result, folded to one line. Opened, it shows the first lines
    /// of its output: the model gets all of it; a chatty job must not push the
    /// conversation off the screen.
    pub fn job_result(&mut self, content: &str) {
        let mut result = JobResult::read(content);
        let mut lines = result.output.lines();
        let mut text = lines
            .by_ref()
            .take(JOB_LINES)
            .collect::<Vec<_>>()
            .join("\n");
        if let Some((end, _)) = text.char_indices().nth(JOB_CHARS) {
            text.truncate(end);
            text.push_str("...");
        }
        match lines.count() {
            0 => {}
            more => text.push_str(&format!("\n[{more} more lines]")),
        }
        result.output = text;
        self.push(Entry::Job {
            result,
            open: false,
        });
    }

    pub fn notice(&mut self, text: impl Into<String>, tone: Tone) {
        self.push(Entry::Notice {
            local_id: None,
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

    /// The newest command still running, which the user may send to the background.
    pub fn running_command(&self) -> Option<String> {
        self.entries.iter().rev().find_map(|entry| match entry {
            Entry::Tool {
                call, result: None, ..
            } if call.name == "run_command" => Some(call.id.clone()),
            _ => None,
        })
    }

    /// The command still running at entry `index`, or in the run under the activity line
    /// there.
    pub fn running_command_at(&self, index: usize) -> Option<String> {
        let running = |entry: &Entry| match entry {
            Entry::Tool {
                call, result: None, ..
            } if call.name == "run_command" => Some(call.id.clone()),
            _ => None,
        };
        match self.entries.get(index)? {
            Entry::Activity { .. } => self.run(index).find_map(running),
            entry => running(entry),
        }
    }

    pub fn start_tool(&mut self, call: ToolCall) {
        self.asked.call(&call);
        self.push(Entry::Tool {
            call,
            result: None,
            open: false,
        });
    }

    pub fn finish_tool(&mut self, call_id: &str, finished: ToolResult) {
        let mut shown = None;
        let running = self
            .entries
            .iter_mut()
            .enumerate()
            .rev()
            .find_map(|(index, entry)| match entry {
                Entry::Tool { call, result, .. } if call.id == call_id && result.is_none() => {
                    shown = Block::shown(call, &finished.content).map(|block| (index, block));
                    Some(result)
                }
                _ => None,
            });
        if let Some(result) = running {
            *result = Some(finished);
        }
        if let Some((index, block)) = shown {
            self.show_block(index, block);
        }
        self.changed();
    }

    /// One change the core reports live. A change seen before (it was undone) takes its
    /// line's place, so a thread never shows one change twice.
    pub fn skill_change(&mut self, change: &SkillChange) {
        match self.skill_index(change.id) {
            Some(index) => {
                self.entries[index] = skill_entry(change);
                self.changed();
            }
            None => self.push(skill_entry(change)),
        }
    }

    /// The changes of a thread read from the core, each placed where it happened in the
    /// history, after the steps that were running then.
    pub fn add_skill_changes(&mut self, changes: &[SkillChange]) {
        for change in changes {
            let at = self
                .times
                .iter()
                .position(|&time| time > change.changed_at_ms)
                .unwrap_or(self.entries.len())
                .min(self.entries.len());
            let mut index = at;
            while self.entries.get(index).is_some_and(is_step) {
                index += 1;
            }
            self.insert(index, skill_entry(change));
        }
    }

    fn skill_index(&self, id: i64) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| matches!(entry, Entry::Skill { id: seen, .. } if *seen == id))
    }

    /// A shown block takes the place of its tool step and stands outside the run of steps:
    /// the steps before it keep their activity line, the steps after it get their own.
    /// Live and from history the calls start in the same order, so the block lands in the
    /// same place both ways.
    fn show_block(&mut self, index: usize, block: Block) {
        self.entries[index] = Entry::Block(block);
        if self.entries.get(index + 1).is_some_and(is_step) {
            self.insert(index + 1, Entry::Activity { open: false });
        }
        let before = index.checked_sub(1);
        if before.is_some_and(|before| matches!(self.entries[before], Entry::Activity { .. })) {
            self.remove(index - 1);
        }
    }

    /// Puts an entry at `index`, as of the entry before it; the hidden ones from there on
    /// move down with it.
    fn insert(&mut self, index: usize, entry: Entry) {
        let at = index
            .checked_sub(1)
            .and_then(|before| self.time(before))
            .unwrap_or_else(dusk_protocol::system::now_ms);
        self.entries.insert(index, entry);
        if index <= self.times.len() {
            self.times.insert(index, at);
        }
        self.hidden = self
            .hidden
            .iter()
            .map(|&hidden| if hidden >= index { hidden + 1 } else { hidden })
            .collect();
        self.changed();
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
                    exit_code: None,
                    artifacts: Vec::new(),
                });
            }
        }
        self.changed();
    }

    /// What the copy button of an entry copies: its whole text, folded or not.
    pub fn text(&self, index: usize) -> Option<String> {
        Some(match self.entries.get(index)? {
            Entry::User { text, .. } => paste::plain(text),
            // What is drawn as markdown copies its labels' words without the braces.
            Entry::Answer(text) | Entry::Summary { text, .. } | Entry::FromAgent { text, .. } => {
                dusk_protocol::thread::unmarked(text)
            }
            Entry::Activity { .. } => String::new(),
            Entry::Reasoning { text, .. }
            | Entry::Notice { text, .. }
            | Entry::Skill { text, .. } => text.clone(),
            Entry::Job { result, .. } => format!("{}\n{}", result.title(), result.output),
            Entry::Block(block) => block.text(),
            Entry::Tool { call, result, .. } => match result {
                Some(result) => result.content.clone(),
                None => call.arguments.to_string(),
            },
        })
    }

    pub fn hide(&mut self, index: usize) {
        self.hidden.insert(index);
        self.changed();
    }

    pub fn is_hidden(&self, index: usize) -> bool {
        self.hidden.contains(&index)
    }

    /// A hidden entry opens like a folded one.
    pub fn is_foldable(&self, index: usize) -> bool {
        if self.is_hidden(index) {
            return true;
        }
        if self.is_folded_away(index) {
            return false;
        }
        match self.entries.get(index) {
            Some(Entry::User { text, .. }) => paste::has_pasted(text),
            entry => matches!(
                entry,
                Some(
                    Entry::Activity { .. }
                        | Entry::Reasoning { .. }
                        | Entry::Tool { .. }
                        | Entry::Summary { .. }
                        | Entry::FromAgent { .. }
                        | Entry::Job { .. }
                )
            ),
        }
    }

    pub fn toggle(&mut self, index: usize) {
        self.changed();
        if self.hidden.remove(&index) {
            return;
        }
        if let Some(
            Entry::User { open, .. }
            | Entry::Activity { open }
            | Entry::Reasoning { open, .. }
            | Entry::Tool { open, .. }
            | Entry::Summary { open, .. }
            | Entry::FromAgent { open, .. }
            | Entry::Job { open, .. },
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
            match entry {
                Entry::Reasoning { open, .. } => *open = !any_open,
                // What is opened has to show.
                Entry::Activity { open } if !any_open => *open = true,
                _ => {}
            }
        }
        self.changed();
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

/// The row a skill change draws as. Its name and reason are the agent's text, so they
/// are shown as plain text: control and bidi characters never reach the screen.
pub fn skill_line(change: &SkillChange) -> String {
    let action = match change.action {
        SkillSaveAction::Created => "created",
        SkillSaveAction::Updated | SkillSaveAction::Unchanged => "improved",
    };
    let skill: String = change
        .skill
        .chars()
        .map(|c| if plain(c) { c } else { '?' })
        .collect();
    // A reason is one line; a line break becomes a space so the words stay apart.
    let reason: String = change
        .reason
        .chars()
        .filter_map(|c| match c {
            '\n' | '\r' | '\t' => Some(' '),
            _ if plain(c) => Some(c),
            _ => None,
        })
        .collect();
    let undone = if change.undone { " (undone)" } else { "" };
    format!("Skill {skill} {action}: {reason}{undone}")
}

fn skill_entry(change: &SkillChange) -> Entry {
    Entry::Skill {
        id: change.id,
        text: skill_line(change),
        tone: if change.undone {
            Tone::Info
        } else {
            Tone::Success
        },
    }
}

/// Whether a character can stand in a row as itself: no control, no bidi override.
fn plain(c: char) -> bool {
    !c.is_control()
        && !matches!(
            c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::catalog::SkillSave;
    use serde_json::{Value, json};

    fn call(id: &str, name: &str, arguments: Value) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            arguments,
        }
    }

    fn flow() -> Value {
        json!({ "flow": { "layers": [
            { "title": "Parse", "cards": [{ "title": "lexer", "line": "tokens" }] },
            { "title": "Check", "emphasis": true },
        ] } })
    }

    /// What `show` answers: the block's text, or the refusal.
    fn result(call: &ToolCall) -> ToolResult {
        let content = Block::asked(call).map_or("error: refused".into(), |block| block.text());
        ToolResult {
            is_error: content.starts_with("error:"),
            content,
            exit_code: None,
            artifacts: Vec::new(),
        }
    }

    fn message(
        role: MessageRole,
        content: &str,
        calls: Vec<ToolCall>,
        answers: Option<&str>,
    ) -> Message {
        Message {
            message_id: String::new(),
            thread_id: String::new(),
            sequence: 0,
            role,
            content: content.into(),
            reasoning: None,
            tool_calls: calls,
            tool_call_id: answers.map(Into::into),
            artifacts: Vec::new(),
            from_thread_id: None,
            from_job_id: None,
            content_tokens: 0,
            tool_calls_tokens: 0,
            reasoning_tokens: 0,
            created_at: 0,
        }
    }

    fn kinds(transcript: &Transcript) -> Vec<&'static str> {
        let kinds = transcript.entries.iter().map(|entry| match entry {
            Entry::User { .. } => "user",
            Entry::Answer(_) => "answer",
            Entry::Activity { .. } => "activity",
            Entry::Tool { .. } => "tool",
            Entry::Block(_) => "block",
            _ => "other",
        });
        kinds.collect()
    }

    /// The same turn live and from history: a message that calls `calls` at once.
    fn both_ways(calls: &[ToolCall]) -> (Transcript, Transcript) {
        let mut live = Transcript::default();
        live.add_user("go");
        live.add_answer("Here it is.");
        for call in calls {
            live.start_tool(call.clone());
        }
        for call in calls {
            live.finish_tool(&call.id, result(call));
        }
        let mut history = vec![
            message(MessageRole::User, "go", Vec::new(), None),
            message(MessageRole::Assistant, "Here it is.", calls.to_vec(), None),
        ];
        for call in calls {
            let content = result(call).content;
            history.push(message(
                MessageRole::Tool,
                &content,
                Vec::new(),
                Some(&call.id),
            ));
        }
        (live, Transcript::from_history(&history))
    }

    #[test]
    fn a_shown_block_is_one_entry_in_place_of_its_step() {
        let (live, reopened) = both_ways(&[call("a", "show", flow())]);
        assert_eq!(kinds(&live), ["user", "answer", "block"]);
        assert_eq!(kinds(&reopened), kinds(&live));
        let shown = Block::asked(&call("a", "show", flow()));
        assert!(matches!(&live.entries[2], Entry::Block(block) if Some(block) == shown.as_ref()));
        assert_eq!(live.text(2), shown.map(|block| block.text()));
    }

    #[test]
    fn steps_around_a_block_keep_their_own_activity_lines() {
        let calls = [
            call("a", "read_file", json!({ "path": "x" })),
            call("b", "show", flow()),
            call("c", "search_files", json!({ "pattern": "y" })),
        ];
        let (live, reopened) = both_ways(&calls);
        let expected = [
            "user", "answer", "activity", "tool", "block", "activity", "tool",
        ];
        assert_eq!(kinds(&live), expected);
        assert_eq!(kinds(&reopened), expected);
        assert_eq!(live.run(2).count(), 1);
        assert_eq!(live.run(5).count(), 1);
    }

    #[test]
    fn a_block_that_was_not_shown_stays_a_step() {
        let out_of_caps = json!({ "flow": { "layers": [{ "title": "only one" }] } });
        let (live, reopened) = both_ways(&[call("a", "show", out_of_caps)]);
        assert_eq!(kinds(&live), ["user", "answer", "activity", "tool"]);
        assert_eq!(kinds(&reopened), kinds(&live));
    }

    #[test]
    fn hidden_entries_follow_the_entries_a_block_moves() {
        let mut transcript = Transcript::default();
        transcript.add_user("go");
        transcript.start_tool(call("a", "show", flow()));
        transcript.start_tool(call("b", "read_file", json!({})));
        transcript.add_answer("after");
        transcript.hide(4);
        transcript.finish_tool("a", result(&call("a", "show", flow())));
        // user, block, activity, tool, answer: the answer is still the hidden one.
        assert_eq!(
            kinds(&transcript),
            ["user", "block", "activity", "tool", "answer"]
        );
        assert!(transcript.is_hidden(4));
        assert_eq!(transcript.times.len(), transcript.entries.len());
    }

    fn change(id: i64, action: SkillSaveAction, undone: bool) -> SkillChange {
        SkillChange {
            id,
            skill: "notes/outline".into(),
            call_id: "c".into(),
            action,
            reason: "adds the outline step".into(),
            changed_at_ms: 0,
            undone,
            can_undo: !undone,
        }
    }

    fn skill_rows(transcript: &Transcript) -> Vec<String> {
        transcript
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Skill { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_old_save_marker_in_a_tool_result_makes_no_row() {
        let mut transcript = Transcript::default();
        let save = SkillSave {
            name: "notes/outline".into(),
            action: SkillSaveAction::Created,
        };
        transcript.start_tool(call("a", "run_command", json!({ "command": "x" })));
        transcript.finish_tool(
            "a",
            ToolResult {
                content: save.stored(),
                is_error: false,
                exit_code: Some(0),
                artifacts: Vec::new(),
            },
        );
        assert!(skill_rows(&transcript).is_empty());
        assert!(
            !transcript
                .entries
                .iter()
                .any(|e| matches!(e, Entry::Notice { .. }))
        );
    }

    #[test]
    fn a_skill_line_names_the_change_and_keeps_untrusted_text_plain() {
        let mut shown = change(1, SkillSaveAction::Created, true);
        assert_eq!(
            skill_line(&shown),
            "Skill notes/outline created: adds the outline step (undone)"
        );
        shown.skill = "a\x1b[2J\u{202e}b".into();
        shown.reason = "first\nsecond\u{2067}".into();
        shown.undone = false;
        shown.action = SkillSaveAction::Updated;
        assert_eq!(skill_line(&shown), "Skill a?[2J?b improved: first second");
    }

    #[test]
    fn a_change_seen_again_replaces_its_line() {
        let mut transcript = Transcript::default();
        transcript.skill_change(&change(1, SkillSaveAction::Updated, false));
        transcript.skill_change(&change(1, SkillSaveAction::Updated, true));
        assert_eq!(
            skill_rows(&transcript),
            ["Skill notes/outline improved: adds the outline step (undone)"]
        );
    }

    #[test]
    fn a_history_change_sits_where_it_happened() {
        let mut first = message(MessageRole::User, "go", Vec::new(), None);
        first.created_at = 10;
        let mut answer = message(MessageRole::Assistant, "done", Vec::new(), None);
        answer.created_at = 30;
        let mut transcript = Transcript::from_history(&[first, answer]);
        let mut early = change(1, SkillSaveAction::Created, false);
        early.changed_at_ms = 20;
        transcript.add_skill_changes(&[early]);
        assert_eq!(kinds(&transcript), ["user", "other", "answer"]);
        assert!(matches!(&transcript.entries[1], Entry::Skill { id: 1, .. }));
    }
}
