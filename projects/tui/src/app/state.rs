//! The client state itself: its fields, how it opens a thread, and its timers.

use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use dusk_core::{
    config::Effort,
    config::live::LiveConfig,
    mcp::McpStatus,
    state::{Goal, LocalUnsent, Message, MessageRole, Task, ThreadTokens},
    tools::JobInfo,
};
use ratatui::layout::Rect;

use super::{
    Activity, ApprovalHit, Asked, ButtonHit, ChipHit, EditorHit, Entry, Focus, GoalHit, Input,
    InputRow, Layout, Panel, PathPopup, PluginRow, Poll, Pressed, QueueHit, Selection,
    ShownPreview, SubagentView, Tone, Transcript, TranscriptHit, panels::agents,
};

pub struct App {
    pub model: String,
    /// The chat's mode: its prompt and tools.
    pub mode: String,
    /// The reasoning effort; `None` leaves it to the provider.
    pub effort: Option<Effort>,
    pub provider_available: bool,
    pub local_unsent: Vec<LocalUnsent>,
    pub plugins: Vec<PluginRow>,
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
    /// Lines scrolled up from the bottom; near 0 it follows new output.
    pub scroll: usize,
    /// Largest transcript offset on the last drawn frame.
    pub max_scroll: usize,
    /// The transcript's line count and width on the last frame, to tell new output.
    pub drawn_size: (usize, usize),
    /// Height of the transcript on the last frame, for paging.
    pub viewport: usize,
    pub transcript_area: Option<Rect>,
    /// Advances while the agent works, for the spinner.
    pub frame: usize,
    /// The idle screen, while it plays.
    pub screensaver: Option<crate::screensaver::Playing>,
    pub pressed: Option<Pressed>,
    /// The folder duskcode runs in, by name, for the status bar.
    pub folder: String,
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
    /// The tasks section on the last frame, and how far it is scrolled.
    pub task_area: Option<Rect>,
    pub task_scroll: usize,
    pub task_max_scroll: usize,
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
    /// Tools waiting for the user's yes, the oldest first; only the first is shown.
    pub approvals: Vec<Asked>,
    pub approval_hits: Vec<ApprovalHit>,
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
    /// The open thread's background jobs as last read; the rail shows the running ones.
    pub jobs: Vec<JobInfo>,
    /// Read the jobs again at once: a tool ended, a result arrived, a thread opened.
    pub jobs_stale: bool,
    pub jobs_read: Instant,
    /// Image previews on screen in the last frame, and the pictures drawn over them
    /// where the terminal can.
    pub previews: Vec<ShownPreview>,
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
    pub(super) turn_running: bool,
    pub(super) compacting: bool,
    pub(super) sent: Vec<String>,
    pub(super) sent_ids: HashSet<String>,
    pub(super) recall: Option<usize>,
    pub(super) recall_draft: Option<Input>,
    pub(super) output_samples: VecDeque<(Instant, usize)>,
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
            plugins: Vec::new(),
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
            drawn_size: (0, 0),
            viewport: 0,
            transcript_area: None,
            frame: 0,
            screensaver: None,
            pressed: None,
            folder: std::env::current_dir()
                .ok()
                .and_then(|dir| {
                    dir.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .unwrap_or_default(),
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
            approvals: Vec::new(),
            approval_hits: Vec::new(),
            forked: None,
            task_area: None,
            task_scroll: 0,
            task_max_scroll: 0,
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
            jobs: Vec::new(),
            jobs_stale: false,
            jobs_read: Instant::now(),
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
            sent_ids: HashSet::new(),
            recall: None,
            recall_draft: None,
            output_samples: VecDeque::new(),
        }
    }

    /// Shows another thread: its history, or nothing for a new one.
    pub fn open(&mut self, thread_id: Option<String>, title: Option<String>, history: &[Message]) {
        if self.thread_id != thread_id {
            self.input = Input::default();
        }
        (self.sent_ids, self.sent) = history
            .iter()
            .filter(|message| {
                message.role == MessageRole::User
                    && message.from_thread_id.is_none()
                    && Some(message.thread_id.as_str()) == thread_id.as_deref()
            })
            .map(|message| (message.message_id.clone(), message.content.clone()))
            .unzip();
        self.recall = None;
        self.recall_draft = None;
        self.jobs.clear();
        self.jobs_stale = true;
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
        self.drawn_size = (0, 0);
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
        self.task_scroll = 0;
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

    pub(super) fn record_output(&mut self, text: &str) {
        self.output_samples.push_back((Instant::now(), text.len()));
        self.trim_output_samples();
    }

    pub(super) fn trim_output_samples(&mut self) {
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

    /// The thread stopped: clear activity and mark unobserved subagent work unavailable.
    pub fn stop_work(&mut self) {
        self.activity = Activity::Idle;
        self.turn_running = false;
        self.compacting = false;
        self.turn_started_at = None;
        self.transcript.stop_running_tools();
        let unavailable_at = now_ms();
        for agent in self.subagents.iter_mut().filter(|agent| agent.is_working()) {
            agent.unavailable_at = Some(unavailable_at);
        }
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
pub(super) fn tenths(value: u64, unit: u64) -> String {
    let tenths = (value * 10 + unit / 2) / unit;
    match tenths % 10 {
        0 => (tenths / 10).to_string(),
        rest => format!("{}.{rest}", tenths / 10),
    }
}
