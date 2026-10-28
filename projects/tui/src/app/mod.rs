//! The client state: the open thread as entries, what is being typed, what the agent is
//! doing and which panel is open. Events from the core and keys from the user change it;
//! anything that needs the core goes back to the session as an `Action`.

mod input;
mod keys;
mod panels;
mod transcript;

use std::{path::PathBuf, sync::Arc};

use dusk_core::{
    live_config::LiveConfig,
    providers::ModelEvent,
    runtime::ThreadEvent,
    state::{Message, ThreadTokens, TurnStatus},
};

pub use input::Input;
pub use panels::{Panel, SettingKind, SettingList, ThreadList};
pub use transcript::{Entry, Tone, Transcript};

/// What the session does for the app.
pub enum Action {
    None,
    Quit,
    Send(String),
    SendNow(String),
    Cancel,
    ShowThreads,
    OpenThread(String),
    NewThread,
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
    /// Height of the transcript on the last frame, for paging.
    pub viewport: usize,
    /// Advances while the agent works, for the spinner.
    pub frame: usize,
    pub config: Arc<LiveConfig>,
    pub tools: Vec<&'static str>,
    pub skills_dir: PathBuf,
    turn_running: bool,
    compacting: bool,
    sent: Vec<String>,
    recall: Option<usize>,
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
            viewport: 0,
            frame: 0,
            config,
            tools,
            skills_dir,
            turn_running: false,
            compacting: false,
            sent: Vec::new(),
            recall: None,
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
        self.turn_running = false;
        self.compacting = false;
    }

    pub fn is_busy(&self) -> bool {
        self.activity != Activity::Idle
    }

    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.transcript.notice(text, Tone::Error);
    }

    pub fn apply(&mut self, event: ThreadEvent) {
        match event {
            ThreadEvent::TurnStarted(_) => {
                self.turn_running = true;
                self.activity = Activity::Thinking;
            }
            ThreadEvent::Model(ModelEvent::Reasoning(text)) => {
                self.transcript.add_reasoning(&text);
                if !self.compacting {
                    self.activity = Activity::Thinking;
                }
            }
            ThreadEvent::Model(ModelEvent::Text(text)) if self.compacting => {
                self.transcript.add_summary(&text);
            }
            ThreadEvent::Model(ModelEvent::Text(text)) => {
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
            ThreadEvent::Idle => {
                self.turn_running = false;
                self.activity = Activity::Idle;
            }
            _ => {}
        }
    }

    /// Takes the typed text for sending; commands show as notices, messages as the user's.
    fn submit(&mut self, now: bool) -> Action {
        if self.input.is_empty() {
            return Action::None;
        }
        let text = self.input.take();
        self.sent.push(text.clone());
        self.recall = None;
        self.scroll = 0;
        if dusk_core::commands::parse(&text).is_some() {
            self.transcript.notice(text.trim().to_owned(), Tone::Info);
        } else {
            self.transcript.push(Entry::User(text.clone()));
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
