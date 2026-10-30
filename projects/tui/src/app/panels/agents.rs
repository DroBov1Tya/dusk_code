//! Subagents as the client sees them: a card for each with its numbers, and a window
//! that shows one agent's thread live. Everything comes from the core's events; the
//! history a window starts from comes from the core as well.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dusk_core::{providers::ModelEvent, runtime::ThreadEvent, state::ThreadTokens};
use ratatui::layout::{Position, Rect};

use crate::app::{App, Entry, Panel, Transcript, TranscriptHit, now_ms};

/// Output older than this no longer counts toward tokens per second.
pub const RATE_WINDOW: Duration = Duration::from_secs(3);
/// Lines a window moves for PgUp, PgDn and the wheel.
const PAGE: usize = 10;
const WHEEL: usize = 3;

/// A subagent the open thread asked, directly or through another agent.
pub struct SubagentView {
    pub thread_id: String,
    /// The agent it works as, from `agents/`.
    pub agent: String,
    pub task: String,
    /// The thread that asked it.
    pub parent: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub tokens: ThreadTokens,
    /// Its largest request.
    pub limit: u64,
    /// Text that came in recently, with when, for tokens per second.
    output: VecDeque<(Instant, usize)>,
}

/// One agent's thread open in a window over the screen.
pub struct AgentWindow {
    pub thread_id: String,
    /// The thread that asked the agent: its messages show as the requests they are.
    parent: Option<String>,
    pub transcript: Transcript,
    /// Lines scrolled up from the bottom; 0 follows new output.
    pub scroll: usize,
    pub max_scroll: usize,
    /// Fold headers on the last drawn frame.
    pub hits: Vec<TranscriptHit>,
    /// The window with its border on the last drawn frame; a click outside closes it.
    pub area: Rect,
}

impl SubagentView {
    pub fn tokens_per_second(&self) -> Option<f64> {
        if self.finished_at.is_some() {
            return None;
        }
        rate(&self.output)
    }

    /// How long it has worked, or worked in all once finished.
    pub fn elapsed_ms(&self) -> u64 {
        let end = self.finished_at.unwrap_or_else(now_ms);
        end.saturating_sub(self.started_at).max(0) as u64
    }

    fn record_output(&mut self, text: &str) {
        let now = Instant::now();
        self.output.push_back((now, text.len()));
        while self
            .output
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > RATE_WINDOW)
        {
            self.output.pop_front();
        }
    }
}

impl AgentWindow {
    pub fn new(thread_id: String, parent: Option<String>, mut transcript: Transcript) -> Self {
        for entry in transcript.entries_mut() {
            if let Entry::FromAgent {
                thread_id, text, ..
            } = entry
                && parent.as_ref() == Some(thread_id)
            {
                *entry = request(std::mem::take(text));
            }
        }
        Self {
            thread_id,
            parent,
            transcript,
            scroll: 0,
            max_scroll: 0,
            hits: Vec::new(),
            area: Rect::default(),
        }
    }

    fn scroll_by(&mut self, lines: usize, up: bool) {
        self.scroll = if up {
            self.scroll.saturating_add(lines).min(self.max_scroll)
        } else {
            self.scroll.saturating_sub(lines)
        };
    }

    /// What the agent's thread does shows in the window as it happens.
    fn apply(&mut self, event: ThreadEvent) {
        let transcript = &mut self.transcript;
        match event {
            ThreadEvent::Model(ModelEvent::Reasoning(text)) => transcript.add_reasoning(&text),
            ThreadEvent::Model(ModelEvent::Text(text)) => transcript.add_answer(&text),
            ThreadEvent::ToolStarted(call) => transcript.start_tool(call),
            ThreadEvent::ToolFinished { call_id, result } => {
                transcript.finish_tool(&call_id, result)
            }
            ThreadEvent::TurnFinished(_) => transcript.stop_running_tools(),
            ThreadEvent::FromAgent(message) => {
                let from = message.from_thread_id.unwrap_or_default();
                let entry = if self.parent.as_ref() == Some(&from) {
                    request(message.content)
                } else {
                    Entry::from_agent(from, message.content)
                };
                transcript.push(entry);
            }
            _ => {}
        }
    }
}

/// What the asking thread wrote, shown like a user's message.
fn request(text: String) -> Entry {
    Entry::User { text, open: false }
}

/// Recent output in tokens per second, at about four bytes a token.
pub fn rate(samples: &VecDeque<(Instant, usize)>) -> Option<f64> {
    let now = Instant::now();
    let first = samples.front()?.0;
    let bytes: usize = samples
        .iter()
        .filter(|(at, _)| now.duration_since(*at) <= RATE_WINDOW)
        .map(|(_, bytes)| *bytes)
        .sum();
    let seconds = now.duration_since(first).as_secs_f64().clamp(0.5, 3.0);
    Some(bytes as f64 / 4.0 / seconds)
}

impl App {
    /// An event of the agent thread `thread_id`: its card, its window if open, and the
    /// output rate of everything at work.
    pub fn apply_subagent(&mut self, thread_id: &str, event: ThreadEvent) {
        match event {
            ThreadEvent::SubagentStarted {
                thread_id: child,
                agent,
                task,
                ..
            } => self.start_subagent(child, agent, task, Some(thread_id.to_owned())),
            ThreadEvent::Subagent {
                thread_id: child,
                event,
            } => self.apply_subagent(&child, *event),
            event => {
                self.update_card(thread_id, &event);
                if let Some(Panel::Agent(window)) = &mut self.panel
                    && window.thread_id == thread_id
                {
                    window.apply(event);
                }
            }
        }
    }

    /// Asking the same thread again brings its card back to work.
    pub fn start_subagent(
        &mut self,
        thread_id: String,
        agent: String,
        task: String,
        parent: Option<String>,
    ) {
        let earlier = self
            .subagents
            .iter()
            .position(|view| view.thread_id == thread_id)
            .map(|at| self.subagents.remove(at));
        let (tokens, limit) = earlier.map_or((ThreadTokens::default(), 0), |view| {
            (view.tokens, view.limit)
        });
        self.subagents.push(SubagentView {
            thread_id,
            agent,
            task,
            parent,
            started_at: now_ms(),
            finished_at: None,
            tokens,
            limit,
            output: VecDeque::new(),
        });
    }

    /// Whether an unfolded task shows a step whose timer runs.
    pub fn task_timers_run(&self) -> bool {
        self.tasks.iter().any(|task| {
            !task.done
                && self.unfolded_tasks.contains(&task.task_id)
                && task
                    .items
                    .iter()
                    .any(|item| item.started_at.is_some() && !item.done)
        })
    }

    pub fn agents_at_work(&self) -> bool {
        self.subagents
            .iter()
            .any(|agent| agent.finished_at.is_none())
    }

    pub fn agent_window_key(&mut self, key: KeyEvent) {
        let Some(Panel::Agent(window)) = &mut self.panel else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.panel = None,
            KeyCode::Up => window.scroll_by(1, true),
            KeyCode::Down => window.scroll_by(1, false),
            KeyCode::PageUp => window.scroll_by(PAGE, true),
            KeyCode::PageDown => window.scroll_by(PAGE, false),
            KeyCode::End => window.scroll = 0,
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                window.transcript.toggle_reasoning()
            }
            _ => {}
        }
    }

    pub fn agent_window_mouse(&mut self, mouse: MouseEvent) {
        let Some(Panel::Agent(window)) = &mut self.panel else {
            return;
        };
        let (x, y) = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::ScrollUp => window.scroll_by(WHEEL, true),
            MouseEventKind::ScrollDown => window.scroll_by(WHEEL, false),
            MouseEventKind::Down(MouseButton::Left)
                if !window.area.contains(Position::new(x, y)) =>
            {
                self.panel = None;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let hit = window
                    .hits
                    .iter()
                    .find(|hit| y == hit.row && x >= hit.left && x < hit.right)
                    .map(|hit| hit.index);
                if let Some(index) = hit {
                    window.transcript.toggle(index);
                }
            }
            _ => {}
        }
    }

    fn update_card(&mut self, thread_id: &str, event: &ThreadEvent) {
        if let ThreadEvent::Model(ModelEvent::Reasoning(text) | ModelEvent::Text(text)) = event {
            self.record_output(text);
        }
        let Some(card) = self
            .subagents
            .iter_mut()
            .find(|view| view.thread_id == thread_id)
        else {
            return;
        };
        match event {
            ThreadEvent::Model(ModelEvent::Reasoning(text) | ModelEvent::Text(text)) => {
                card.record_output(text)
            }
            ThreadEvent::Tokens { tokens, limit } => {
                card.tokens = *tokens;
                card.limit = *limit;
            }
            ThreadEvent::TurnFinished(_) | ThreadEvent::Failed(_) if card.finished_at.is_none() => {
                card.finished_at = Some(now_ms());
            }
            _ => {}
        }
    }
}
