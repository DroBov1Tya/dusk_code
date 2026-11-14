//! What one chat shows, built from its history and then from its live events. Plain
//! data, so the drawing code only reads it.

use std::{collections::BTreeMap, time::Instant};

use dusk_protocol::{
    model::ModelEvent,
    thread::{
        AgentState, DETAIL_KEYS, Message, MessageRole, Note, Task, ThreadEvent, ThreadTokens, Tool,
        ToolState, clip, note,
    },
    tools::{ApprovalRequest, Card, Question},
};

pub enum Item {
    /// `sequence` is the point /undo goes back to; only the user's own messages have one.
    User {
        text: String,
        sequence: Option<u64>,
    },
    /// A shell block the user ran from an answer; `result` is empty while it runs.
    Command(CommandRun),
    /// What a subagent reported back: the agent's side of the chat, not the user's.
    Agent {
        from: String,
        text: String,
    },
    Assistant {
        text: String,
        reasoning: String,
    },
    Tool(Tool),
    /// A picture a tool or a message carries, by its ID in the pictures folder.
    Image(Picture),
    /// A sound a tool carries, to play in the chat.
    Audio(Clip),
    Notice(String),
    Error(String),
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

/// A subagent of the chat, as the right column and the agents window show it.
pub struct Agent {
    /// The name drawn from its thread, such as `Ember-0c`.
    pub name: String,
    /// The agent it works as, such as `general`.
    pub role: String,
    pub task: String,
    pub working: bool,
    pub state: AgentState,
    /// The tools it runs now, by call, oldest first: several may run at once.
    pub running: Vec<(String, String)>,
    pub started: Instant,
    pub ended: Option<Instant>,
}

impl Agent {
    pub fn run_time(&self) -> std::time::Duration {
        self.ended.unwrap_or_else(Instant::now) - self.started
    }

    /// The user stopped it.
    pub fn stop(&mut self) {
        self.working = false;
        self.state = AgentState::Done;
        self.running.clear();
        self.ended = self.ended.or_else(|| Some(Instant::now()));
    }

    fn follow(&mut self, event: &ThreadEvent) {
        // An agent that ended starts again only with a new turn: what trails in after a
        // stop must not bring it back to work.
        if !self.working && !matches!(event, ThreadEvent::TurnStarted(_)) {
            return;
        }
        let state = match event {
            ThreadEvent::TurnStarted(_) => {
                self.running.clear();
                AgentState::Thinking
            }
            ThreadEvent::ToolStarted(call) => {
                self.running.push((call.id.clone(), call.name.clone()));
                AgentState::Tool(call.name.clone())
            }
            // One of several tools ended: the fox goes on with the newest still running.
            ThreadEvent::ToolFinished { call_id, .. } => {
                self.running.retain(|(id, _)| id != call_id);
                match self.running.last() {
                    Some((_, name)) => AgentState::Tool(name.clone()),
                    None => AgentState::Thinking,
                }
            }
            // The model speaks between tools, not over them.
            ThreadEvent::Model(ModelEvent::Reasoning(_)) if self.running.is_empty() => {
                AgentState::Thinking
            }
            ThreadEvent::Model(ModelEvent::Text(_)) if self.running.is_empty() => {
                AgentState::Writing
            }
            ThreadEvent::Idle => AgentState::Done,
            ThreadEvent::Failed(_) => AgentState::Failed,
            _ => return,
        };
        if matches!(state, AgentState::Done | AgentState::Failed) {
            self.running.clear();
        }
        self.working = !matches!(state, AgentState::Done | AgentState::Failed);
        self.ended = match self.working {
            true => None,
            false => self.ended.or_else(|| Some(Instant::now())),
        };
        self.state = state;
    }
}

#[derive(Default)]
pub struct Transcript {
    pub items: Vec<Item>,
    pub activity_boundaries: Vec<usize>,
    /// When each boundary was drawn, in Unix milliseconds, beside `activity_boundaries`.
    pub boundary_times: Vec<i64>,
    /// What the thread had spent, as tokens and micro-dollars, when each boundary was
    /// drawn; a turn's own spend is the difference across it.
    pub boundary_spend: Vec<(u64, u64)>,
    /// The thread's spend as the core last reported it.
    spent: (u64, u64),
    /// When the last turn seen live ended, for the header's star.
    pub finished_at: Option<Instant>,
    pub working: bool,
    pub tokens: Option<(u64, u64)>,
    /// What the chat cost so far, when any call's cost is known.
    pub cost: Option<String>,
    /// Screenshots the model took to see the screen, oldest first: they go to the right
    /// column, not into the chat, which keeps the pictures it chose to show.
    pub screenshots: Vec<Picture>,
    /// The chat's own access mode; `None` follows the default in settings.
    pub access: Option<dusk_protocol::config::ApprovalPolicy>,
    pub tasks: Vec<Task>,
    pub agents: BTreeMap<String, Agent>,
    pub approvals: Vec<ApprovalRequest>,
    /// What the agent asked and nobody answered yet, in order; the first one shows.
    pub questions: std::collections::VecDeque<Question>,
    /// The answers given so far; they go to the agent together, after the last one.
    pub answers: Vec<String>,
    pub title: Option<String>,
    /// Text the core gave back, such as after /undo, for the composer to offer again.
    pub offered: Option<String>,
    /// Where the items are cut once the core confirms a rollback.
    pub rollback_at: Option<usize>,
    next_run: u64,
    /// What each subagent of the chat said and did, by its thread ID.
    pub agent_chats: BTreeMap<String, Transcript>,
    /// What passed between the agents and the arbiter, oldest first.
    pub exchanges: Vec<Exchange>,
    /// When the arbiter last answered, for its pulse.
    pub arbiter_at: Option<Instant>,
}

impl Transcript {
    /// Answers the question shown. Gives the message for the agent once every question
    /// asked has its answer: a second question must not take the place of the first.
    pub fn answer(&mut self, chosen: &[usize], own: &str) -> Option<String> {
        let question = self.questions.pop_front()?;
        self.answers.push(question.answer(chosen, own));
        self.questions
            .is_empty()
            .then(|| std::mem::take(&mut self.answers).join("\n\n"))
    }

    pub fn from_history(history: &[Message]) -> Self {
        let mut transcript = Self::default();
        let mut question = None;
        for message in history {
            if message.role == MessageRole::User && message.from_thread_id.is_none() {
                // The core admits only a matching answer into an active question.
                // It persists as a User message, so recognize the stored call and
                // its exact encoded prompt together.
                let is_answer = question.is_some_and(|prompt| {
                    message
                        .content
                        .starts_with(&format!("QUESTION: {prompt}\nANSWER: "))
                });
                if !is_answer {
                    transcript.activity_boundary(message.created_at);
                }
                question = None;
            }
            transcript.add_message(message);
            // Persisted model steps carry calls; the final response is a separate
            // assistant message. A new human prompt also ends legacy partial turns.
            if message.role == MessageRole::Assistant {
                if message.tool_calls.is_empty() {
                    transcript.activity_boundary(message.created_at);
                    question = None;
                } else {
                    question = message
                        .tool_calls
                        .iter()
                        .find(|call| call.name == "ask_user")
                        .and_then(|call| call.arguments.get("question"))
                        .and_then(serde_json::Value::as_str);
                }
            }
        }
        transcript
    }

    fn add_message(&mut self, message: &Message) {
        match message.role {
            MessageRole::System => {}
            MessageRole::User if message.from_thread_id.is_some() => {
                let from = message.from_thread_id.as_deref().unwrap_or_default();
                self.items.push(Item::Agent {
                    from: self.agent_name(from),
                    text: message.content.clone(),
                })
            }
            MessageRole::User | MessageRole::Job => self.items.push(Item::User {
                text: message.content.clone(),
                sequence: (message.role == MessageRole::User).then_some(message.sequence),
            }),
            MessageRole::Assistant => {
                if !message.content.is_empty() || message.reasoning.is_some() {
                    self.items.push(Item::Assistant {
                        text: message.content.clone(),
                        reasoning: message.reasoning.clone().unwrap_or_default(),
                    });
                }
                for call in &message.tool_calls {
                    self.items.push(Item::Tool(tool(call, ToolState::Done)));
                }
            }
            MessageRole::Tool => {
                if let Some(tool) =
                    self.tool_mut(message.tool_call_id.as_deref().unwrap_or_default())
                {
                    tool.output = clip(&message.content);
                }
            }
        }
        let call_id = message.tool_call_id.as_deref().unwrap_or_default();
        self.add_pictures(call_id, &message.artifacts);
    }

    /// Pictures a tool returned: those `display_image` shows go into the chat, the rest
    /// are screenshots for the right column.
    fn add_pictures(&mut self, call_id: &str, artifacts: &[Card]) {
        let shown = self
            .tool_mut(call_id)
            .is_none_or(|tool| tool.name == "display_image");
        for artifact in artifacts {
            if artifact.kind == dusk_protocol::tools::CardKind::Audio {
                self.items.push(Item::Audio(artifact.into()));
                continue;
            }
            match shown {
                true => self.items.push(Item::Image(artifact.into())),
                false => self.screenshots.push(artifact.into()),
            }
        }
    }

    pub fn apply(&mut self, event: ThreadEvent) {
        if let Some(note) = note(&event) {
            match &event {
                ThreadEvent::Failed(_) => self.working = false,
                ThreadEvent::ArbiterDecided {
                    what,
                    allow,
                    reason,
                } => self.decided(CHAT, what, *allow, reason),
                _ => {}
            }
            self.items.push(match note {
                Note::Notice(text) => Item::Notice(text),
                Note::Error(text) => Item::Error(text),
            });
            return;
        }
        match event {
            ThreadEvent::FromAgent(message) => {
                let from = message
                    .from_thread_id
                    .as_deref()
                    .map_or_else(|| "an agent".to_owned(), |id| self.agent_name(id));
                self.exchange(&from, CHAT, ExchangeKind::Reported, &message.content);
                self.items.push(Item::Agent {
                    from,
                    text: message.content,
                });
            }
            ThreadEvent::UserMessage(message) | ThreadEvent::FromJob(message) => {
                let sequence = (message.role == MessageRole::User).then_some(message.sequence);
                self.items.push(Item::User {
                    text: message.content,
                    sequence,
                })
            }
            ThreadEvent::TurnStarted(_) => {
                self.activity_boundary(dusk_protocol::system::now_ms());
                self.working = true;
            }
            ThreadEvent::TurnFinished(_) | ThreadEvent::Idle => {
                self.activity_boundary(dusk_protocol::system::now_ms());
                if self.working {
                    self.finished_at = Some(Instant::now());
                }
                self.working = false;
            }
            ThreadEvent::Model(ModelEvent::Text(text)) => self.assistant().0.push_str(&text),
            ThreadEvent::Model(ModelEvent::Reasoning(text)) => self.assistant().1.push_str(&text),
            ThreadEvent::Model(_) => {}
            ThreadEvent::ToolStarted(call) => {
                self.items.push(Item::Tool(tool(&call, ToolState::Running)));
            }
            ThreadEvent::ToolFinished { call_id, result } => {
                if let Some(tool) = self.tool_mut(&call_id) {
                    tool.finish(&result);
                }
                self.add_pictures(&call_id, &result.artifacts);
            }
            ThreadEvent::Tokens { tokens, limit } => {
                self.tokens = Some((context(&tokens), limit));
                self.cost = tokens.cost();
                let used = tokens.spent.prompt_tokens + tokens.spent.completion_tokens;
                self.spent = (used, tokens.cost_micro_usd);
            }
            ThreadEvent::ModelSelected { input_limit, .. } => {
                let used = self.tokens.map_or(0, |(used, _)| used);
                self.tokens = Some((used, input_limit));
            }
            ThreadEvent::Tasks(tasks) => self.tasks = tasks,
            ThreadEvent::SubagentStarted {
                thread_id,
                agent,
                task,
                ..
            } => {
                let name = dusk_protocol::thread::agent_name(&thread_id);
                self.exchange(CHAT, &name, ExchangeKind::Asked, &task);
                self.agents.insert(
                    thread_id,
                    Agent {
                        name,
                        role: agent,
                        task,
                        working: true,
                        state: AgentState::Thinking,
                        running: Vec::new(),
                        started: Instant::now(),
                        ended: None,
                    },
                );
            }
            ThreadEvent::Subagent { thread_id, event } => {
                // What an agent asks, and what is answered for it, shows with its name.
                let who = self.agent_name(&thread_id);
                match &*event {
                    ThreadEvent::SubagentStarted { agent, task, .. } => {
                        self.exchange(&who, agent, ExchangeKind::Asked, task);
                    }
                    ThreadEvent::Approval(request) => {
                        self.exchange(
                            &who,
                            ARBITER,
                            ExchangeKind::Approval,
                            &request.request.what(),
                        );
                    }
                    ThreadEvent::ArbiterDecided {
                        what,
                        allow,
                        reason,
                    } => {
                        self.decided(&who, what, *allow, reason);
                    }
                    _ => {}
                }
                if let Some(agent) = self.agents.get_mut(&thread_id) {
                    agent.follow(&event);
                }
                self.agent_chats.entry(thread_id).or_default().apply(*event);
            }
            ThreadEvent::Question(question) => self.questions.push_back(question),
            ThreadEvent::Approval(request) => {
                self.exchange(
                    CHAT,
                    ARBITER,
                    ExchangeKind::Approval,
                    &request.request.what(),
                );
                self.approvals.push(request);
            }
            ThreadEvent::ApprovalClosed { id } => self.approvals.retain(|request| request.id != id),
            ThreadEvent::TitleChanged { title } => self.title = Some(title),
            ThreadEvent::Undone { text } => {
                // The core forgot the message and all after it; so does the view.
                if let Some(at) = self.rollback_at.take() {
                    self.items.truncate(at);
                    let kept = self
                        .activity_boundaries
                        .iter()
                        .take_while(|boundary| **boundary <= at)
                        .count();
                    self.activity_boundaries.truncate(kept);
                    self.boundary_times.truncate(kept);
                    self.boundary_spend.truncate(kept);
                }
                self.offered = Some(text);
            }
            // The rest left a note above or is not shown; the rate waits for its UI.
            _ => {}
        }
    }

    /// How many times each tool ran in the chat, most used first.
    pub fn activity(&self) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for item in &self.items {
            if let Item::Tool(tool) = item {
                *counts.entry(tool.name.as_str()).or_default() += 1;
            }
        }
        let mut counts: Vec<_> = counts
            .into_iter()
            .map(|(name, count)| (name.to_owned(), count))
            .collect();
        counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        counts
    }

    fn agent_name(&self, thread_id: &str) -> String {
        self.agents
            .get(thread_id)
            .map_or_else(|| "an agent".to_owned(), |agent| agent.name.clone())
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

    fn tool_mut(&mut self, call_id: &str) -> Option<&mut Tool> {
        self.items.iter_mut().rev().find_map(|item| match item {
            Item::Tool(tool) if tool.call_id == call_id => Some(tool),
            _ => None,
        })
    }

    /// Starts showing a command the user ran; returns its ID for `finish_run`.
    pub fn start_run(&mut self, command: String) -> u64 {
        self.next_run += 1;
        self.items.push(Item::Command(CommandRun {
            id: self.next_run,
            command,
            started: Instant::now(),
            result: None,
        }));
        self.next_run
    }

    pub fn finish_run(&mut self, id: u64, result: Result<String, String>) {
        for item in &mut self.items {
            if let Item::Command(run) = item
                && run.id == id
            {
                run.result = Some(result);
                return;
            }
        }
    }

    fn activity_boundary(&mut self, at: i64) {
        let end = self.items.len();
        if end > 0 && self.activity_boundaries.last() != Some(&end) {
            self.activity_boundaries.push(end);
            self.boundary_times.push(at);
            self.boundary_spend.push(self.spent);
        }
    }

    /// When the turn whose items end at boundary `index` began and ended; the turn after
    /// the last boundary is still open.
    /// What the turn ending at boundary `index` spent, as tokens and micro-dollars; none
    /// for a turn read from history, whose spend was not seen.
    pub fn turn_spend(&self, index: usize) -> Option<(u64, u64)> {
        let before = *self.boundary_spend.get(index.checked_sub(1)?)?;
        let after = *self.boundary_spend.get(index)?;
        let spent = (
            after.0.saturating_sub(before.0),
            after.1.saturating_sub(before.1),
        );
        (spent.0 > 0).then_some(spent)
    }

    /// The mean cost of the turns that have one, to tell an unusually dear turn.
    pub fn mean_turn_cost(&self) -> Option<u64> {
        let costs: Vec<u64> = (1..self.boundary_spend.len())
            .filter_map(|index| self.turn_spend(index))
            .map(|(_, micro)| micro)
            .filter(|micro| *micro > 0)
            .collect();
        (!costs.is_empty()).then(|| costs.iter().sum::<u64>() / costs.len() as u64)
    }

    pub fn span(&self, index: usize) -> Option<super::timing::Span> {
        let began = *self.boundary_times.get(index.checked_sub(1)?)?;
        Some((began, self.boundary_times.get(index).copied()))
    }

    /// The answer being written: the last item, or a new one after a tool or a message.
    fn assistant(&mut self) -> (&mut String, &mut String) {
        if self.activity_boundaries.last() == Some(&self.items.len())
            || !matches!(self.items.last(), Some(Item::Assistant { .. }))
        {
            self.items.push(Item::Assistant {
                text: String::new(),
                reasoning: String::new(),
            });
        }
        match self.items.last_mut() {
            Some(Item::Assistant { text, reasoning }) => (text, reasoning),
            _ => unreachable!("an answer was just pushed"),
        }
    }
}

fn context(tokens: &ThreadTokens) -> u64 {
    tokens.context
}

/// A step as the shared transcript makes it; a skill, loaded by its name, shows that
/// name where no other argument says what the tool was asked.
fn tool(call: &dusk_protocol::model::ToolCall, state: ToolState) -> Tool {
    let mut tool = Tool::new(call, state);
    let has_detail = DETAIL_KEYS.iter().any(|key| {
        call.arguments
            .get(key)
            .is_some_and(serde_json::Value::is_string)
    });
    if !has_detail && let Some(name) = call.arguments.get("name").and_then(|value| value.as_str()) {
        tool.detail = name
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(160)
            .collect();
    }
    tool
}
