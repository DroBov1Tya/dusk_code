//! The client state: the open thread as entries, what is being typed, what the agent is
//! doing and which panel is open. Events from the core and keys from the user change it;
//! anything that needs the core goes back to the session as an `Action`.

mod editor;
mod input;
mod keys;
mod panels;
pub mod paste;
mod transcript;

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use dusk_core::{
    chat::commands::{self, COMMANDS, Command},
    live_config::LiveConfig,
    providers::ModelEvent,
    runtime::ThreadEvent,
    state::{Message, ThreadTokens, TurnStatus},
};
use ratatui::layout::Rect;

pub use editor::{EditorButton, EditorHit, PasteEditor};
pub use input::Input;
pub use panels::{ModelList, Panel, Preview, SettingKind, SettingList, ThreadList};
pub use transcript::{Entry, Tone, Transcript};

/// What the session does for the app.
pub enum Action {
    None,
    Quit,
    Send(String),
    SendNow(String),
    Cancel,
    /// A command the core answers for the client, such as the threads for /resume.
    Command(Command),
    OpenThread(String),
    NewThread,
    Copy(String),
    /// Load the thread the threads panel points at, for its preview.
    Preview(String),
}

/// Every command the box offers, as the core lists them.
pub fn commands() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    COMMANDS.iter()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Input,
    /// Walking the foldable entries; the index is the selected one.
    Browse(usize),
}

#[derive(Clone, PartialEq, Eq)]
pub enum Activity {
    Idle,
    Thinking,
    Writing,
    Tool(String),
    Compacting,
    Retrying(u32),
}

/// A button on the entry under the mouse, on the last drawn frame.
pub struct ButtonHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub index: usize,
    pub button: Button,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Copy,
    /// Puts a sent message back into the box.
    Edit,
}

/// A paste chip in the message box on the last drawn frame.
pub struct ChipHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub index: usize,
}

/// Text marked with the mouse, as (transcript line, screen column) at both ends.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: (usize, u16),
    pub cursor: (usize, u16),
}

impl Selection {
    /// Both ends, top first.
    pub fn ordered(self) -> ((usize, u16), (usize, u16)) {
        if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }
}

/// Transcript columns left of this hold bars and indents, not text, so a selection
/// starts after them.
pub const TEXT_COLUMN: u16 = 3;

/// The header row of a foldable entry on the last drawn frame.
pub struct TranscriptHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub index: usize,
}

/// A subagent the open thread asked, by the thread it works in.
pub struct SubagentView {
    pub thread_id: String,
    /// The agent it works as, from `agents/`.
    pub agent: String,
    pub task: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

const RATE_WINDOW: Duration = Duration::from_secs(3);

pub struct App {
    pub model: String,
    pub thread_id: Option<String>,
    pub title: Option<String>,
    pub transcript: Transcript,
    pub input: Input,
    pub focus: Focus,
    pub panel: Option<Panel>,
    pub activity: Activity,
    pub tokens: ThreadTokens,
    /// The largest request, until the core reports its own.
    pub limit: u64,
    /// Lines scrolled up from the bottom; 0 follows new output.
    pub scroll: usize,
    /// Largest transcript offset on the last drawn frame.
    pub max_scroll: usize,
    /// Height of the transcript on the last frame, for paging.
    pub viewport: usize,
    pub transcript_area: Option<Rect>,
    /// Advances while the agent works, for the spinner.
    pub frame: usize,
    /// Hidden by F2 when there is room.
    pub sidebar_visible: bool,
    pub sidebar_overlay_open: bool,
    pub compact_layout: bool,
    pub sidebar_overlay: Option<Rect>,
    pub sidebar_area: Option<Rect>,
    pub agent_area: Option<Rect>,
    pub mcp_area: Option<Rect>,
    pub agent_scroll: usize,
    pub mcp_scroll: usize,
    pub agent_max_scroll: usize,
    pub mcp_max_scroll: usize,
    pub transcript_hits: Vec<TranscriptHit>,
    pub button_hits: Vec<ButtonHit>,
    pub input_chips: Vec<ChipHit>,
    pub editor_hits: Vec<EditorHit>,
    /// The entry under the mouse, which shows its buttons.
    pub hover: Option<usize>,
    pub selection: Option<Selection>,
    /// Every transcript line as plain text on the last frame, for copying a selection.
    pub drawn_text: Vec<String>,
    /// The first transcript line on screen on the last frame.
    pub drawn_top: usize,
    /// The lines each entry took on the last frame, from and to.
    pub entry_lines: Vec<(usize, usize)>,
    /// A short word for the status rule, such as "copied", until the next input.
    pub flash: Option<String>,
    /// When Ctrl+C last cleared the box; another soon after quits.
    pub quit_armed: Option<Instant>,
    pub command_popup: Option<Rect>,
    pub subagents: Vec<SubagentView>,
    pub active_mcp: Vec<String>,
    pub turn_started_at: Option<i64>,
    pub last_turn_duration_ms: Option<u64>,
    pub config: Arc<LiveConfig>,
    pub tools: Vec<&'static str>,
    pub skills_dir: PathBuf,
    turn_running: bool,
    compacting: bool,
    sent: Vec<String>,
    recall: Option<usize>,
    output_samples: VecDeque<(Instant, usize)>,
}

impl App {
    pub fn new(
        model: String,
        limit: u64,
        config: Arc<LiveConfig>,
        tools: Vec<&'static str>,
        skills_dir: PathBuf,
    ) -> Self {
        Self {
            model,
            thread_id: None,
            title: None,
            transcript: Transcript::default(),
            input: Input::default(),
            focus: Focus::Input,
            panel: None,
            activity: Activity::Idle,
            tokens: ThreadTokens::default(),
            limit,
            scroll: 0,
            max_scroll: 0,
            viewport: 0,
            transcript_area: None,
            frame: 0,
            sidebar_visible: true,
            sidebar_overlay_open: false,
            compact_layout: false,
            sidebar_overlay: None,
            sidebar_area: None,
            agent_area: None,
            mcp_area: None,
            agent_scroll: 0,
            mcp_scroll: 0,
            agent_max_scroll: 0,
            mcp_max_scroll: 0,
            transcript_hits: Vec::new(),
            button_hits: Vec::new(),
            input_chips: Vec::new(),
            editor_hits: Vec::new(),
            hover: None,
            selection: None,
            drawn_text: Vec::new(),
            drawn_top: 0,
            entry_lines: Vec::new(),
            flash: None,
            quit_armed: None,
            command_popup: None,
            subagents: Vec::new(),
            active_mcp: Vec::new(),
            turn_started_at: None,
            last_turn_duration_ms: None,
            config,
            tools,
            skills_dir,
            turn_running: false,
            compacting: false,
            sent: Vec::new(),
            recall: None,
            output_samples: VecDeque::new(),
        }
    }

    /// Shows another thread: its history, or nothing for a new one.
    pub fn open(&mut self, thread_id: Option<String>, title: Option<String>, history: &[Message]) {
        self.thread_id = thread_id;
        self.title = title;
        self.transcript = Transcript::from_history(history);
        self.focus = Focus::Input;
        self.activity = Activity::Idle;
        self.tokens = ThreadTokens::default();
        self.scroll = 0;
        self.max_scroll = 0;
        self.transcript_area = None;
        self.transcript_hits.clear();
        self.button_hits.clear();
        self.hover = None;
        self.selection = None;
        self.command_popup = None;
        self.sidebar_overlay = None;
        self.sidebar_area = None;
        self.agent_area = None;
        self.mcp_area = None;
        self.agent_scroll = 0;
        self.mcp_scroll = 0;
        self.agent_max_scroll = 0;
        self.mcp_max_scroll = 0;
        self.subagents.clear();
        self.active_mcp.clear();
        self.turn_started_at = None;
        self.last_turn_duration_ms = None;
        self.output_samples.clear();
        self.turn_running = false;
        self.compacting = false;
    }

    pub fn is_busy(&self) -> bool {
        self.activity != Activity::Idle
    }

    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.trim_output_samples();
    }

    /// Recent visible output divided by four bytes per estimated token.
    pub fn approximate_tokens_per_second(&self) -> Option<f64> {
        if !self.is_busy() {
            return None;
        }
        let now = Instant::now();
        let first = self.output_samples.front()?.0;
        let bytes: usize = self
            .output_samples
            .iter()
            .filter(|(at, _)| now.duration_since(*at) <= RATE_WINDOW)
            .map(|(_, bytes)| *bytes)
            .sum();
        let seconds = now.duration_since(first).as_secs_f64().clamp(0.5, 3.0);
        Some(bytes as f64 / 4.0 / seconds)
    }

    pub fn elapsed_ms(&self) -> Option<u64> {
        self.turn_started_at
            .map(|started| now_ms().saturating_sub(started).max(0) as u64)
            .or(self.last_turn_duration_ms)
    }

    fn record_output(&mut self, text: &str) {
        self.output_samples.push_back((Instant::now(), text.len()));
        self.trim_output_samples();
    }

    fn trim_output_samples(&mut self) {
        let now = Instant::now();
        while self
            .output_samples
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > RATE_WINDOW)
        {
            self.output_samples.pop_front();
        }
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.transcript.notice(text, Tone::Error);
    }

    pub fn apply(&mut self, event: ThreadEvent) {
        match event {
            ThreadEvent::TurnStarted(turn) => {
                self.turn_running = true;
                self.activity = Activity::Thinking;
                self.turn_started_at = Some(turn.started_at);
                self.last_turn_duration_ms = None;
                self.output_samples.clear();
            }
            ThreadEvent::Model(ModelEvent::Reasoning(text)) => {
                self.record_output(&text);
                self.transcript.add_reasoning(&text);
                if !self.compacting {
                    self.activity = Activity::Thinking;
                }
            }
            ThreadEvent::Model(ModelEvent::Text(text)) if self.compacting => {
                self.record_output(&text);
                self.transcript.add_summary(&text);
            }
            ThreadEvent::Model(ModelEvent::Text(text)) => {
                self.record_output(&text);
                self.transcript.add_answer(&text);
                self.activity = Activity::Writing;
            }
            ThreadEvent::ToolStarted(call) => {
                self.activity = Activity::Tool(call.name.clone());
                self.transcript.start_tool(call);
            }
            ThreadEvent::ToolFinished { call_id, result } => {
                self.transcript.finish_tool(&call_id, result);
                self.activity = Activity::Thinking;
            }
            ThreadEvent::TurnFinished(turn) => {
                self.turn_running = false;
                self.last_turn_duration_ms = turn
                    .finished_at
                    .map(|finished| finished.saturating_sub(turn.started_at).max(0) as u64);
                self.turn_started_at = None;
                self.output_samples.clear();
                self.transcript.stop_running_tools();
                match turn.status {
                    TurnStatus::Interrupted => self.transcript.notice("stopped", Tone::Info),
                    TurnStatus::Failed => {
                        let error = turn.error_message.unwrap_or_else(|| "turn failed".into());
                        self.error(error);
                    }
                    _ => {}
                }
            }
            ThreadEvent::Retrying { attempt, error } => {
                self.activity = Activity::Retrying(attempt);
                let text = format!("attempt {attempt} failed, trying again: {error}");
                self.transcript.notice(text, Tone::Info);
            }
            ThreadEvent::TitleChanged { title } => self.title = Some(title),
            ThreadEvent::OutputLimitReached { limit } => {
                let text = format!("the answer reached the output limit of {limit} tokens");
                self.transcript.notice(text, Tone::Info);
            }
            ThreadEvent::CompactionStarted => {
                self.compacting = true;
                self.activity = Activity::Compacting;
            }
            ThreadEvent::CompactionFinished(compaction) => {
                self.compacting = false;
                self.after_compaction();
                let text = format!(
                    "context compacted: messages up to #{} now live in the summary above",
                    compaction.through_sequence
                );
                self.transcript.notice(text, Tone::Info);
            }
            ThreadEvent::CompactionFailed { error } => {
                self.compacting = false;
                self.after_compaction();
                self.error(format!("compaction failed: {error}"));
            }
            ThreadEvent::Tokens { tokens, limit } => {
                self.tokens = tokens;
                self.limit = limit;
            }
            ThreadEvent::ContextTooLarge { tokens, limit } => self.error(format!(
                "the context is ~{} tokens, past the limit of {}: compact it with /compact",
                compact_number(tokens),
                compact_number(limit)
            )),
            ThreadEvent::Rejected { reason } => self.error(reason),
            ThreadEvent::Failed(error) => self.error(format!("thread stopped: {error}")),
            ThreadEvent::SubagentStarted {
                thread_id,
                agent,
                task,
                ..
            } => self.start_subagent(thread_id, agent, task),
            ThreadEvent::Subagent { thread_id, event } => self.apply_subagent(&thread_id, *event),
            ThreadEvent::FromAgent(message) => {
                let from = message.from_thread_id.unwrap_or_default();
                self.transcript
                    .push(Entry::from_agent(from, message.content));
            }
            ThreadEvent::Idle => {
                self.turn_running = false;
                self.activity = Activity::Idle;
                self.turn_started_at = None;
                self.output_samples.clear();
            }
            _ => {}
        }
    }

    /// A subagent's own thread shows only on its card; its output counts in the rate.
    fn apply_subagent(&mut self, thread_id: &str, event: ThreadEvent) {
        match event {
            ThreadEvent::SubagentStarted {
                thread_id,
                agent,
                task,
                ..
            } => self.start_subagent(thread_id, agent, task),
            ThreadEvent::Subagent { thread_id, event } => self.apply_subagent(&thread_id, *event),
            ThreadEvent::Model(ModelEvent::Reasoning(text) | ModelEvent::Text(text)) => {
                self.record_output(&text)
            }
            ThreadEvent::TurnFinished(_) | ThreadEvent::Failed(_) => {
                self.finish_subagent(thread_id)
            }
            _ => {}
        }
    }

    /// Asking the same thread again brings its card back to work.
    fn start_subagent(&mut self, thread_id: String, agent: String, task: String) {
        self.subagents.retain(|view| view.thread_id != thread_id);
        self.subagents.push(SubagentView {
            thread_id,
            agent,
            task,
            started_at: now_ms(),
            finished_at: None,
        });
    }

    fn finish_subagent(&mut self, thread_id: &str) {
        let now = now_ms();
        for agent in &mut self.subagents {
            if agent.finished_at.is_none() && agent.thread_id == thread_id {
                agent.finished_at = Some(now);
            }
        }
    }

    pub fn agents_at_work(&self) -> bool {
        self.subagents
            .iter()
            .any(|agent| agent.finished_at.is_none())
    }

    /// Takes the typed text for sending; commands show as notices, messages as the user's.
    fn submit(&mut self, now: bool) -> Action {
        if self.input.is_empty() {
            return Action::None;
        }
        let text = self.input.take();
        self.sent.push(text.clone());
        self.recall = None;
        // The core says what a command is; ones that do not run in the thread come
        // back from it as something to show.
        let command = commands::parse(&text);
        if let Some(Ok(command)) = &command
            && !command.runs_in_thread()
        {
            return Action::Command(command.clone());
        }
        self.scroll = 0;
        if command.is_some() {
            self.transcript.notice(text.trim().to_owned(), Tone::Info);
        } else {
            self.transcript.push(Entry::User {
                text: text.clone(),
                open: false,
            });
        }
        if now {
            Action::SendNow(text)
        } else {
            Action::Send(text)
        }
    }

    /// Steps through what was sent before, newest first.
    fn recall(&mut self, older: bool) {
        let next = match (self.recall, older) {
            (None, true) => self.sent.len().checked_sub(1),
            (None, false) => None,
            (Some(at), true) => Some(at.saturating_sub(1)),
            (Some(at), false) => (at + 1 < self.sent.len()).then_some(at + 1),
        };
        self.recall = next;
        match next {
            Some(at) => self.input.set(self.sent[at].clone()),
            None => {
                self.input.take();
            }
        }
    }

    fn after_compaction(&mut self) {
        self.activity = if self.turn_running {
            Activity::Thinking
        } else {
            Activity::Idle
        };
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

/// 1234 as "1.2k", 100000 as "100k", 1234567 as "1.2M".
pub fn compact_number(value: u64) -> String {
    match value {
        0..1_000 => value.to_string(),
        1_000..1_000_000 => format!("{}k", tenths(value, 1_000)),
        _ => format!("{}M", tenths(value, 1_000_000)),
    }
}

/// `value / unit` with one decimal, dropped when it is zero.
fn tenths(value: u64, unit: u64) -> String {
    let tenths = (value * 10 + unit / 2) / unit;
    match tenths % 10 {
        0 => (tenths / 10).to_string(),
        rest => format!("{}.{rest}", tenths / 10),
    }
}
