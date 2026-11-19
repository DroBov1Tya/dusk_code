//! What one chat shows: the shared chat of the protocol, folded from its history and then
//! from its live events, with what only this window keeps beside it (commands the user ran,
//! the arbiter's exchanges, when rows arrived, the state of skill rows and questions).

use std::{
    collections::{BTreeMap, VecDeque},
    ops::{Deref, DerefMut},
    time::Instant,
};

use dusk_protocol::{
    system::now_ms,
    thread::{ChatLog, Message, ThreadEvent, transcript},
    tools::{Card, Question},
};

/// An item of the chat; a command the user ran from an answer is this window's own.
pub type Item = transcript::Item<CommandRun>;
/// The chat as the protocol folds it.
pub type Log = ChatLog<CommandRun>;

/// A subagent of the chat, as the right column and the agents window show it.
pub use dusk_protocol::thread::Subagent as Agent;

/// How long `agent` worked, until now while it still does.
pub fn run_time(agent: &Agent) -> std::time::Duration {
    std::time::Duration::from_millis(agent.run_ms(now_ms()).max(0) as u64)
}

pub struct CommandRun {
    pub id: u64,
    pub command: String,
    pub started: Instant,
    pub result: Option<Result<String, String>>,
}

/// A sound card: its ID in the artifact folder, how it is labeled and how long it plays.
pub struct Clip {
    pub id: String,
    pub title: Option<String>,
    pub mime: String,
    pub duration: Option<std::time::Duration>,
}

impl From<&Card> for Clip {
    fn from(card: &Card) -> Self {
        Self {
            id: card.id.clone(),
            title: card.title.clone(),
            mime: card.mime_type.clone(),
            duration: (card.duration_ms > 0)
                .then(|| std::time::Duration::from_millis(card.duration_ms)),
        }
    }
}

impl Clip {
    /// A sound the user sent, by its card and the path it was sent as: named by its file,
    /// its kind told by its extension, as the core took only these.
    pub fn sent(id: &str, path: &str) -> Self {
        let path = std::path::Path::new(path);
        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
        let mime = match extension.as_deref() {
            Some("mp3") => "audio/mpeg",
            Some("wav") => "audio/wav",
            Some("ogg") => "audio/ogg",
            Some("flac") => "audio/flac",
            Some("m4a") => "audio/mp4",
            _ => "",
        };
        Self {
            id: id.to_owned(),
            title: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
            mime: mime.to_owned(),
            duration: None,
        }
    }
}

pub struct Picture {
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
}

impl From<&Card> for Picture {
    fn from(artifact: &Card) -> Self {
        Self {
            id: artifact.id.clone(),
            title: artifact.title.clone(),
            description: artifact.description.clone(),
        }
    }
}

/// One thing that passed between the agents, the user's approvals and the arbiter, as
/// the arbiter window shows it.
pub struct Exchange {
    pub at: Instant,
    pub from: String,
    pub to: String,
    pub kind: ExchangeKind,
    pub text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExchangeKind {
    /// A task given to an agent.
    Asked,
    /// An agent's report back.
    Reported,
    /// A tool's request for a yes.
    Approval,
    Allowed,
    Denied,
}

/// Who a chat is in the arbiter window.
pub const CHAT: &str = "chat";
const ARBITER: &str = "arbiter";

/// One chat as this window holds it: the shared chat, and what only the window keeps.
#[derive(Default)]
pub struct Transcript {
    pub log: Log,
    /// When the last turn seen live ended, for the header's star.
    pub finished_at: Option<Instant>,
    /// The chat's own access mode; `None` follows the default in settings.
    pub access: Option<dusk_protocol::config::ApprovalPolicy>,
    /// What each skill change's row shows of its requests, by change ID.
    pub skill_rows: BTreeMap<i64, super::messages::SkillRow>,
    /// What the agent asked and nobody answered yet, in order; the first one shows.
    pub questions: VecDeque<Question>,
    /// The answers given so far; they go to the agent together, after the last one.
    pub answers: Vec<String>,
    /// What passed between the agents and the arbiter, oldest first.
    pub exchanges: Vec<Exchange>,
    /// When the arbiter last answered, for its pulse.
    pub arbiter_at: Option<Instant>,
    /// Items stored before the chat was opened: they are there at once, the later ones
    /// arrive with a short fade.
    pub loaded: usize,
    /// When each later item was first drawn, so it fades in once and never again when
    /// the list scrolls back to it.
    pub arrived: BTreeMap<usize, Instant>,
    next_run: u64,
}

impl Deref for Transcript {
    type Target = Log;

    fn deref(&self) -> &Log {
        &self.log
    }
}

impl DerefMut for Transcript {
    fn deref_mut(&mut self) -> &mut Log {
        &mut self.log
    }
}

impl Transcript {
    /// Answers the question shown. Gives the message for the agent once every question
    /// asked has its answer: a second question must not take the place of the first.
    pub fn answer(&mut self, chosen: &[usize], own: &str) -> Option<String> {
        let question = self.questions.pop_front()?;
        self.answers.push(question.answer(chosen, own));
        if self.questions.is_empty() {
            self.log.live.answered();
        }
        self.questions
            .is_empty()
            .then(|| std::mem::take(&mut self.answers).join("\n\n"))
    }

    pub fn from_history(history: &[Message]) -> Self {
        let log = Log::from_history(history);
        Self {
            loaded: log.items.len(),
            log,
            ..Self::default()
        }
    }

    pub fn apply(&mut self, event: ThreadEvent) {
        self.exchanges_of(&event);
        let ended = matches!(event, ThreadEvent::TurnFinished(_) | ThreadEvent::Idle);
        let was_working = self.log.working;
        match &event {
            ThreadEvent::Question(question) => self.questions.push_back(question.clone()),
            ThreadEvent::Undone { .. } => {
                if let Some(at) = self.log.rollback_at {
                    self.arrived.retain(|index, _| *index < at);
                }
            }
            ThreadEvent::SkillChanged(change) => self.skill_settled(change.id),
            _ => {}
        }
        self.log.apply(event, now_ms());
        if ended && was_working {
            self.finished_at = Some(Instant::now());
        }
    }

    /// What passed between the agents and the arbiter, as the arbiter window shows it.
    fn exchanges_of(&mut self, event: &ThreadEvent) {
        match event {
            ThreadEvent::ArbiterDecided {
                what,
                allow,
                reason,
            } => self.decided(CHAT, what, *allow, reason),
            ThreadEvent::FromAgent(message) => {
                let from = message
                    .from_thread_id
                    .as_deref()
                    .map_or_else(|| "an agent".to_owned(), |id| self.log.agent_name(id));
                self.exchange(&from, CHAT, ExchangeKind::Reported, &message.content);
            }
            ThreadEvent::SubagentStarted {
                thread_id, task, ..
            } => {
                let name = dusk_protocol::thread::agent_name(thread_id);
                self.exchange(CHAT, &name, ExchangeKind::Asked, task);
            }
            ThreadEvent::Approval(request) => {
                let what = request.request.what();
                self.exchange(CHAT, ARBITER, ExchangeKind::Approval, &what);
            }
            // What an agent asks, and what is answered for it, shows with its name.
            ThreadEvent::Subagent { thread_id, event } => {
                let who = self.log.agent_name(thread_id);
                match &**event {
                    ThreadEvent::SubagentStarted { agent, task, .. } => {
                        self.exchange(&who, agent, ExchangeKind::Asked, task);
                    }
                    ThreadEvent::Approval(request) => {
                        let what = request.request.what();
                        self.exchange(&who, ARBITER, ExchangeKind::Approval, &what);
                    }
                    ThreadEvent::ArbiterDecided {
                        what,
                        allow,
                        reason,
                    } => self.decided(&who, what, *allow, reason),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// A skill change that came live, or one that was undone: it replaces the change of
    /// its ID, and its row's request is over.
    pub fn skill_changed(&mut self, change: dusk_protocol::catalog::SkillChange) {
        self.skill_settled(change.id);
        self.log.skill_changed(change);
    }

    fn skill_settled(&mut self, id: i64) {
        if let Some(row) = self.skill_rows.get_mut(&id) {
            row.busy = false;
            row.error = None;
        }
    }

    /// The row state of the skill change `id`, made when first asked for.
    pub fn skill_row(&mut self, id: i64) -> &mut super::messages::SkillRow {
        self.skill_rows.entry(id).or_default()
    }

    fn exchange(&mut self, from: &str, to: &str, kind: ExchangeKind, text: &str) {
        // Each request to the arbiter and each answer sends a wave through its sphere.
        if kind != ExchangeKind::Asked && kind != ExchangeKind::Reported {
            self.arbiter_at = Some(Instant::now());
        }
        self.exchanges.push(Exchange {
            at: Instant::now(),
            from: from.to_owned(),
            to: to.to_owned(),
            kind,
            text: text.chars().take(400).collect(),
        });
    }

    fn decided(&mut self, asker: &str, what: &str, allow: bool, reason: &str) {
        let kind = if allow {
            ExchangeKind::Allowed
        } else {
            ExchangeKind::Denied
        };
        self.exchange(ARBITER, asker, kind, &format!("{what}: {reason}"));
    }

    /// Starts showing a command the user ran; returns its ID for `finish_run`.
    pub fn start_run(&mut self, command: String) -> u64 {
        self.next_run += 1;
        let run = CommandRun {
            id: self.next_run,
            command,
            started: Instant::now(),
            result: None,
        };
        self.log.push(Item::Local(run), now_ms());
        self.next_run
    }

    pub fn finish_run(&mut self, id: u64, result: Result<String, String>) {
        let at = self
            .log
            .items
            .iter()
            .position(|item| matches!(item, Item::Local(run) if run.id == id));
        if let Some(Item::Local(run)) = at.and_then(|at| self.log.item_mut(at)) {
            run.result = Some(result);
        }
    }
}
