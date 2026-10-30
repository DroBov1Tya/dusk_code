//! The client state: the open thread as entries, what is being typed, what the agent is
//! doing and which panel is open. Events from the core and keys from the user change it;
//! anything that needs the core goes back to the session as an `Action`.

mod composer;
mod keys;
mod thread;
pub use keys::EFFORTS;
mod panels;

use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use dusk_core::{
    chat::commands::{self, COMMANDS, Command},
    config::live::LiveConfig,
    config::{ContextPolicy, Effort},
    mcp::McpStatus,
    providers::ModelEvent,
    runtime::ThreadEvent,
    state::{Goal, LocalUnsent, Message, Task, ThreadTokens, TurnStatus},
};
use ratatui::layout::Rect;

pub use composer::PathPopup;
pub use composer::{EditorButton, EditorHit, Input, PasteEditor, paste};
use panels::agents;
pub use panels::{
    AgentRow, AgentWindow, Connection, FileButton, FileEditor, HitTarget, McpRow, ModelList, Panel,
    Preview, SettingCategory, SettingsHit, SettingsView, SubagentView, SwitchKind, ThreadList,
};
pub use thread::goal::{GoalButton, GoalHit};
pub use thread::poll::{Poll, PollHit, PollTarget};
pub use thread::queue::{QueueButton, QueueHit};
pub use thread::transcript::{Entry, ImageSpot, Layout, Tone, Transcript};

/// What the session does for the app.
pub enum Action {
    None,
    Quit,
    Send(String),
    SendNow(String),
    Cancel,
    Connect {
        base_url: String,
        api_key: Option<String>,
    },
    SetAgentLimit(usize),
    SetGoalRounds(u32),
    SetMemoryChars(usize),
    SetContextPolicy(ContextPolicy),
    /// Turn an agent, a tool, a skill or an MCP server on or off.
    Switch {
        kind: SwitchKind,
        name: String,
        on: bool,
    },
    /// Read a skill from disk into the settings, dropping unsaved edits.
    OpenSkill(String),
    SaveSkill {
        name: String,
        text: String,
    },
    /// Read an agent's AGENT.md into the settings, dropping unsaved edits.
    OpenAgentFile(String),
    SaveAgentFile {
        name: String,
        text: String,
    },
    /// Read an MCP server's YAML into the settings; `None` starts a new one.
    OpenMcp(Option<String>),
    /// Save an MCP server in place of `old`; empty text removes it.
    SaveMcp {
        old: Option<String>,
        text: String,
    },
    RestartMcp(String),
    Resend(String),
    /// A command the core answers for the client, such as the threads for /resume.
    Command(Command),
    OpenThread(String),
    /// Download a release and put it in place of the running binary.
    InstallUpdate(String),
    /// Move the chat to its next mode.
    NextMode,
    /// Stop one subagent and the agents it asked.
    StopAgent(String),
    Copy(String),
    /// Open the window of the subagent working in this thread.
    OpenAgent(String),
    /// Load the thread the threads panel points at, for its preview.
    Preview(String),
}

/// Every command the box offers, as the core lists them.
pub fn commands() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    const LOCAL: &[(&str, &str)] = &[
        (
            "/connect",
            "add a provider with its URL and optional API key",
        ),
        (
            "/resend",
            "view local unsent messages and explicitly send one",
        ),
    ];
    COMMANDS.iter().chain(LOCAL.iter())
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
    /// /fork is describing the thread for a new one.
    Forking,
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
    /// Folds any entry into one line, wherever it is scrolled to.
    Hide,
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

/// A row of the message box on the last drawn frame: where each character starts on
/// screen with its byte offset, and the offset a click past the right end goes to.
pub struct InputRow {
    pub row: u16,
    pub right: u16,
    pub cells: Vec<(u16, usize)>,
    pub end: usize,
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

pub struct App {
    pub model: String,
    /// The chat's mode: its prompt and tools.
    pub mode: String,
    /// The reasoning effort; `None` leaves it to the provider.
    pub effort: Option<Effort>,
    pub provider_available: bool,
    pub local_unsent: Vec<LocalUnsent>,
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
    /// The connectors section shows each MCP server; folded, a summary.
    pub connectors_open: bool,
    /// Each open task's block on the last drawn frame, to unfold it with a click.
    pub task_hits: Vec<(Rect, String)>,
    /// Tasks unfolded to show their steps.
    pub unfolded_tasks: Vec<String>,
    pub agent_scroll: usize,
    pub mcp_scroll: usize,
    pub agent_max_scroll: usize,
    pub mcp_max_scroll: usize,
    pub transcript_hits: Vec<TranscriptHit>,
    pub button_hits: Vec<ButtonHit>,
    pub input_chips: Vec<ChipHit>,
    pub input_rows: Vec<InputRow>,
    pub input_area: Option<Rect>,
    /// A drag that started in the message box selects there.
    pub input_dragging: bool,
    pub editor_hits: Vec<EditorHit>,
    /// Messages written while the agent works, oldest first.
    pub queued: Vec<String>,
    pub queue_hits: Vec<QueueHit>,
    /// A /fork summary waiting for its new thread.
    pub forked: Option<String>,
    /// /undo took the thread back to this message: the history is read again and the
    /// text goes back into the box.
    pub undone: Option<String>,
    /// A question the agent asked, until it is answered or declined.
    pub poll: Option<Poll>,
    /// The entry under the mouse, which shows its buttons.
    pub hover: Option<usize>,
    pub selection: Option<Selection>,
    /// The transcript lines of the earlier entries, kept between frames.
    pub layout: Option<Layout>,
    /// The lines of the last entry on the last frame, as plain text.
    pub tail_text: Vec<String>,
    /// The first transcript line on screen on the last frame.
    pub drawn_top: usize,
    /// The lines each entry took on the last frame, from and to.
    pub entry_lines: Vec<(usize, usize)>,
    /// A short word for the status rule, such as "copied", until the next input.
    pub flash: Option<String>,
    /// The repository the client runs in, for the line under the box.
    pub git: Option<crate::host::git::GitSummary>,
    /// When Ctrl+C last cleared the box; another soon after quits.
    pub quit_armed: Option<Instant>,
    pub command_popup: Option<Rect>,
    /// Image previews wholly on screen in the last frame, and the pictures drawn over
    /// them where the terminal can.
    pub previews: Vec<(Rect, dusk_core::state::artifacts::ImageArtifact)>,
    pub pictures: crate::host::graphics::Pictures,
    /// What the folder of a path being typed holds.
    pub paths: Option<PathPopup>,
    pub subagents: Vec<SubagentView>,
    /// The agent's task board in the open thread.
    pub tasks: Vec<Task>,
    /// A newer release the core found in the background, such as `v0.66.5`.
    pub update_available: Option<String>,
    /// The open thread's standing goal.
    pub goal: Option<Goal>,
    /// Rounds a goal may run before it pauses, as the config says.
    pub goal_rounds: u32,
    pub goal_hits: Vec<GoalHit>,
    /// The MCP servers of the config, as the core last reported them.
    pub mcp: Vec<McpStatus>,
    pub turn_started_at: Option<i64>,
    pub last_turn_duration_ms: Option<u64>,
    pub config: Arc<LiveConfig>,
    turn_running: bool,
    compacting: bool,
    sent: Vec<String>,
    recall: Option<usize>,
    output_samples: VecDeque<(Instant, usize)>,
}

impl App {
    /// A transcript line as drawn on the last frame, as plain text.
    pub fn drawn_text(&self, line: usize) -> Option<&str> {
        let earlier = self
            .layout
            .as_ref()
            .map_or(&[][..], |layout| &layout.text[..]);
        match earlier.get(line) {
            Some(text) => Some(text),
            None => self.tail_text.get(line - earlier.len()).map(String::as_str),
        }
    }

    pub fn new(model: Option<String>, limit: u64, config: Arc<LiveConfig>) -> Self {
        Self {
            provider_available: model.is_some(),
            model: model.unwrap_or_else(|| "no model".into()),
            effort: None,
            mode: String::new(),
            local_unsent: Vec::new(),
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
            connectors_open: false,
            task_hits: Vec::new(),
            unfolded_tasks: Vec::new(),
            agent_scroll: 0,
            mcp_scroll: 0,
            agent_max_scroll: 0,
            mcp_max_scroll: 0,
            transcript_hits: Vec::new(),
            button_hits: Vec::new(),
            input_chips: Vec::new(),
            input_rows: Vec::new(),
            input_area: None,
            input_dragging: false,
            editor_hits: Vec::new(),
            queued: Vec::new(),
            queue_hits: Vec::new(),
            goal: None,
            update_available: None,
            undone: None,
            paths: None,
            goal_rounds: config.current().limits().max_goal_rounds,
            goal_hits: Vec::new(),
            poll: None,
            forked: None,
            hover: None,
            selection: None,
            layout: None,
            tail_text: Vec::new(),
            drawn_top: 0,
            entry_lines: Vec::new(),
            flash: None,
            git: None,
            quit_armed: None,
            command_popup: None,
            previews: Vec::new(),
            pictures: Default::default(),
            subagents: Vec::new(),
            tasks: Vec::new(),
            mcp: Vec::new(),
            turn_started_at: None,
            last_turn_duration_ms: None,
            config,
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
        self.poll = None;
        self.local_unsent.clear();
        let images = std::mem::take(&mut self.transcript.images);
        self.transcript = Transcript::from_history(history);
        self.transcript.images = images;
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
        self.task_hits.clear();
        self.unfolded_tasks.clear();
        self.agent_scroll = 0;
        self.mcp_scroll = 0;
        self.agent_max_scroll = 0;
        self.mcp_max_scroll = 0;
        self.subagents.clear();
        self.tasks.clear();
        self.goal = None;
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
    /// Output of the agent and of the subagents at work, while anything works.
    pub fn approximate_tokens_per_second(&self) -> Option<f64> {
        if !self.is_busy() && !self.agents_at_work() {
            return None;
        }
        agents::rate(&self.output_samples)
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
            .is_some_and(|(at, _)| now.duration_since(*at) > agents::RATE_WINDOW)
        {
            self.output_samples.pop_front();
        }
    }

    pub fn error(&mut self, text: impl Into<String>) {
        let text = text.into();
        tracing::warn!("{text}");
        self.transcript.notice(text, Tone::Error);
    }

    pub fn apply(&mut self, event: ThreadEvent) {
        match event {
            ThreadEvent::ModelSelected {
                name,
                context_window,
            } => {
                self.model = name;
                self.provider_available = true;
                self.limit = context_window
                    .map_or(self.config.current().limits().max_input_tokens, |window| {
                        window.min(self.config.current().limits().max_input_tokens)
                    });
            }
            ThreadEvent::LocalUnsent(message) => {
                if self.local_unsent.iter().any(|item| item.id == message.id) {
                    return;
                }
                // Another key may have arrived before this persistence event.
                if let Some(index) = self.transcript.entries.iter().rposition(
                    |entry| matches!(entry, Entry::User { text, .. } if *text == message.content),
                ) {
                    self.transcript.remove(index);
                    self.focus = Focus::Input;
                }
                self.show_local_unsent(message);
            }
            ThreadEvent::ProviderUnavailable => {
                self.provider_available = false;
                self.transcript.notice(
                    "Provider unavailable. You can continue reading this chat; use /connect or /model to choose a provider.",
                    Tone::Info,
                );
            }
            ThreadEvent::TurnStarted(turn) => {
                self.turn_running = true;
                self.activity = Activity::Thinking;
                self.turn_started_at = Some(turn.started_at);
                self.last_turn_duration_ms = None;
                self.output_samples.clear();
            }
            ThreadEvent::Model(ModelEvent::Reasoning(text)) => {
                self.provider_available = true;
                self.record_output(&text);
                self.transcript.add_reasoning(&text);
                if !self.compacting {
                    self.activity = Activity::Thinking;
                }
            }
            ThreadEvent::Model(ModelEvent::Text(text)) if self.compacting => {
                self.provider_available = true;
                self.record_output(&text);
                self.transcript.add_summary(&text);
            }
            ThreadEvent::Model(ModelEvent::Text(text)) => {
                self.provider_available = true;
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
            ThreadEvent::Forked { summary } => self.forked = Some(summary),
            ThreadEvent::Undone { text } => self.undone = Some(text),
            ThreadEvent::Failed(error) => {
                self.error(format!("thread stopped: {error}"));
                self.stop_work();
            }
            ThreadEvent::SubagentStarted {
                thread_id,
                agent,
                task,
                ..
            } => {
                let parent = self.thread_id.clone();
                self.start_subagent(thread_id, agent, task, parent);
            }
            ThreadEvent::Subagent { thread_id, event } => self.apply_subagent(&thread_id, *event),
            ThreadEvent::FromAgent(message) => {
                let from = message.from_thread_id.unwrap_or_default();
                self.transcript
                    .push(Entry::from_agent(from, message.content));
            }
            ThreadEvent::Tasks(tasks) => self.tasks = tasks,
            ThreadEvent::Goal {
                goal,
                note,
                started,
            } => self.goal_changed(goal, note, started),
            ThreadEvent::Question(question) => self.poll = Some(Poll::new(question)),
            ThreadEvent::AnswerCut { error } => self.error(format!(
                "the answer broke off ({error}); what it wrote is kept"
            )),
            ThreadEvent::Idle => {
                self.turn_running = false;
                self.activity = Activity::Idle;
                self.turn_started_at = None;
                self.output_samples.clear();
            }
            _ => {}
        }
    }

    /// The thread stopped: nothing runs any more, whatever the last event said.
    pub fn stop_work(&mut self) {
        self.activity = Activity::Idle;
        self.turn_running = false;
        self.compacting = false;
        self.turn_started_at = None;
        self.transcript.stop_running_tools();
    }

    pub fn show_local_unsent(&mut self, message: LocalUnsent) {
        if self.local_unsent.iter().any(|item| item.id == message.id) {
            return;
        }
        self.transcript.push(Entry::Notice {
            local_id: Some(message.id.clone()),
            text: format!(
                "Saved locally \u{00B7} not sent \u{00B7} /resend\n{}",
                message.content
            ),
            tone: Tone::Info,
        });
        self.local_unsent.push(message);
    }

    pub fn mark_local_sent(&mut self, id: &str) {
        self.local_unsent.retain(|item| item.id != id);
        for entry in self.transcript.entries_mut() {
            if let Entry::Notice {
                local_id: Some(local_id),
                text,
                ..
            } = entry
                && local_id == id
            {
                *text = text.replacen(
                    "Saved locally \u{00B7} not sent \u{00B7} /resend",
                    "Queued by explicit resend",
                    1,
                );
            }
        }
        if let Some(Panel::Unsent { selected }) = &mut self.panel {
            *selected = (*selected).min(self.local_unsent.len().saturating_sub(1));
        }
    }

    /// A message the core refused goes back into the box instead of looking sent.
    pub fn unsend(&mut self, text: String) {
        if let Some(Entry::User { text: shown, .. }) = self.transcript.entries.last()
            && *shown == text
        {
            self.transcript.entries_mut().pop();
        }
        if self.input.is_empty() {
            self.input.set(text);
        }
    }

    /// Takes the typed text for sending; commands show as notices, messages as the user's.
    fn submit(&mut self, now: bool) -> Action {
        if self.input.is_empty() {
            return Action::None;
        }
        let text = self.input.take();
        let trimmed = text.trim();
        match trimmed {
            "/connect" => {
                self.panel = Some(Panel::Connect(Connection::default()));
                return Action::None;
            }
            "/resend" => {
                self.panel = Some(Panel::Unsent { selected: 0 });
                return Action::None;
            }
            _ => {}
        }
        // Connection secrets belong only in the masked form, never in recall/history.
        if trimmed.split_whitespace().next() == Some("/connect") {
            self.panel = Some(Panel::Connect(Connection::default()));
            self.flash = Some("Enter connection details in the form".into());
            return Action::None;
        }
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
        // A message written instead of answering the question declines it.
        let text = match self.poll.take() {
            Some(poll) if command.is_none() => poll.question.declined(&text),
            poll => {
                self.poll = poll;
                text
            }
        };
        if let Some(Ok(Command::Fork { .. })) = &command {
            self.activity = Activity::Forking;
        }
        if command.is_some() {
            self.scroll = 0;
            self.transcript.notice(text.trim().to_owned(), Tone::Info);
        } else if self.is_busy() && !now {
            self.queued.push(text);
            return Action::None;
        } else {
            self.show_sent(&text);
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
