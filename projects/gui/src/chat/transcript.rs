//! What one chat shows, built from its history and then from its live events. Plain
//! data, so the drawing code only reads it.

use std::{collections::BTreeMap, time::Instant};

use dusk_protocol::{
    artifact::ImageArtifact,
    conversation::{Message, MessageRole},
    model::ModelEvent,
    thread::ThreadEvent,
    tools::{ApprovalRequest, Question},
    work::{Task, ThreadTokens},
};

pub enum Item {
    User(String),
    Assistant {
        text: String,
        reasoning: String,
    },
    Tool(Tool),
    /// A picture a tool or a message carries, by its ID in the pictures folder.
    Image(Picture),
    Notice(String),
    Error(String),
}

pub struct Picture {
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
}

impl From<&ImageArtifact> for Picture {
    fn from(artifact: &ImageArtifact) -> Self {
        Self {
            id: artifact.id.clone(),
            title: artifact.title.clone(),
            description: artifact.description.clone(),
        }
    }
}

pub struct Tool {
    pub call_id: String,
    pub name: String,
    pub detail: String,
    /// The arguments in full, for the unfolded step.
    pub arguments: String,
    /// The start of what the tool answered, once it has.
    pub output: String,
    pub state: ToolState,
}

/// How much of a tool's arguments and answer an unfolded step shows.
const SHOWN_CHARS: usize = 4000;

fn clip(text: &str) -> String {
    let mut clipped: String = text.chars().take(SHOWN_CHARS).collect();
    if clipped.len() < text.len() {
        clipped.push_str("\n...");
    }
    clipped
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Running,
    Done,
    Failed,
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

/// A subagent of the chat, as the right column shows it.
pub struct Agent {
    pub name: String,
    pub task: String,
    pub working: bool,
}

#[derive(Default)]
pub struct Transcript {
    pub items: Vec<Item>,
    pub working: bool,
    pub tokens: Option<(u64, u64)>,
    pub tasks: Vec<Task>,
    pub agents: BTreeMap<String, Agent>,
    pub approvals: Vec<ApprovalRequest>,
    pub question: Option<Question>,
    pub title: Option<String>,
    /// Text the core gave back, such as after /undo, for the composer to offer again.
    pub offered: Option<String>,
    /// What each subagent of the chat said and did, by its thread ID.
    pub agent_chats: BTreeMap<String, Transcript>,
    /// What passed between the agents and the arbiter, oldest first.
    pub exchanges: Vec<Exchange>,
    /// When the arbiter last answered, for its pulse.
    pub arbiter_at: Option<Instant>,
}

impl Transcript {
    pub fn from_history(history: &[Message]) -> Self {
        let mut transcript = Self::default();
        for message in history {
            transcript.add_message(message);
        }
        transcript
    }

    fn add_message(&mut self, message: &Message) {
        match message.role {
            MessageRole::System => {}
            MessageRole::User | MessageRole::Job => {
                self.items.push(Item::User(message.content.clone()))
            }
            MessageRole::Assistant => {
                if !message.content.is_empty() || message.reasoning.is_some() {
                    self.items.push(Item::Assistant {
                        text: message.content.clone(),
                        reasoning: message.reasoning.clone().unwrap_or_default(),
                    });
                }
                for call in &message.tool_calls {
                    self.items.push(Item::Tool(Tool {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        detail: summary(&call.arguments),
                        arguments: clip(&call.arguments.to_string()),
                        output: String::new(),
                        state: ToolState::Done,
                    }));
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
        for artifact in &message.artifacts {
            self.items.push(Item::Image(artifact.into()));
        }
    }

    pub fn apply(&mut self, event: ThreadEvent) {
        match event {
            ThreadEvent::FromAgent(message) => {
                let from = message
                    .from_thread_id
                    .as_deref()
                    .map_or_else(|| "an agent".to_owned(), |id| self.agent_name(id));
                self.exchange(&from, CHAT, ExchangeKind::Reported, &message.content);
                self.items.push(Item::User(message.content));
            }
            ThreadEvent::UserMessage(message) | ThreadEvent::FromJob(message) => {
                self.items.push(Item::User(message.content))
            }
            ThreadEvent::TurnStarted(_) => self.working = true,
            ThreadEvent::TurnFinished(_) | ThreadEvent::Idle => self.working = false,
            ThreadEvent::Model(ModelEvent::Text(text)) => self.assistant().0.push_str(&text),
            ThreadEvent::Model(ModelEvent::Reasoning(text)) => self.assistant().1.push_str(&text),
            ThreadEvent::Model(_) => {}
            ThreadEvent::ToolStarted(call) => {
                self.items.push(Item::Tool(Tool {
                    detail: summary(&call.arguments),
                    arguments: clip(&call.arguments.to_string()),
                    output: String::new(),
                    call_id: call.id,
                    name: call.name,
                    state: ToolState::Running,
                }));
            }
            ThreadEvent::ToolFinished { call_id, result } => {
                if let Some(tool) = self.tool_mut(&call_id) {
                    tool.state = if result.is_error {
                        ToolState::Failed
                    } else {
                        ToolState::Done
                    };
                    tool.output = clip(&result.content);
                }
                for artifact in &result.artifacts {
                    self.items.push(Item::Image(artifact.into()));
                }
            }
            ThreadEvent::Tokens { tokens, limit } => self.tokens = Some((context(&tokens), limit)),
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
                self.exchange(CHAT, &agent, ExchangeKind::Asked, &task);
                self.agents.insert(
                    thread_id,
                    Agent {
                        name: agent,
                        task,
                        working: true,
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
                    match &*event {
                        ThreadEvent::TurnStarted(_) => agent.working = true,
                        ThreadEvent::Idle | ThreadEvent::Failed(_) => agent.working = false,
                        _ => {}
                    }
                }
                self.agent_chats.entry(thread_id).or_default().apply(*event);
            }
            ThreadEvent::Question(question) => self.question = Some(question),
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
                self.notice("Went back to an earlier message.");
                self.offered = Some(text);
            }
            ThreadEvent::Failed(error) => {
                self.working = false;
                self.items.push(Item::Error(error));
            }
            ThreadEvent::Rejected { reason } => self.items.push(Item::Error(reason)),
            ThreadEvent::ProviderUnavailable => self.items.push(Item::Error(
                "The provider is unavailable; check the connection in the settings.".into(),
            )),
            ThreadEvent::Retrying { attempt, error } => {
                self.notice(&format!("Retrying ({attempt}): {error}"));
            }
            ThreadEvent::AnswerCut { error } => self.items.push(Item::Error(error)),
            ThreadEvent::CompactionFailed { error } => {
                self.items
                    .push(Item::Error(format!("Compaction failed: {error}")));
            }
            ThreadEvent::OutputLimitReached => self.notice("The answer reached the output limit."),
            ThreadEvent::PicturesRefused { model } => {
                self.notice(&format!("{model} does not take pictures."));
            }
            ThreadEvent::CompactionStarted => self.notice("Compacting the context..."),
            ThreadEvent::CompactionFinished(_) => self.notice("The context was compacted."),
            ThreadEvent::ContextTooLarge { tokens, limit } => self.items.push(Item::Error(
                format!("The context is too large: {tokens} of {limit} tokens."),
            )),
            ThreadEvent::ArbiterDecided {
                what,
                allow,
                reason,
            } => {
                let verdict = if allow { "allowed" } else { "denied" };
                self.notice(&format!("Arbiter {verdict} {what}: {reason}"));
                self.decided(CHAT, &what, allow, &reason);
            }
            ThreadEvent::Goal { note, .. } => self.notice(&note),
            ThreadEvent::Forked { .. } => self.notice("Forked into a new chat."),
            ThreadEvent::AgentPhase { .. }
            | ThreadEvent::LocalUnsent(_)
            | ThreadEvent::Queued { .. }
            // Ignored until the rate UI lands.
            | ThreadEvent::OutputRate { .. } => {}
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
        self.arbiter_at = Some(Instant::now());
    }

    fn tool_mut(&mut self, call_id: &str) -> Option<&mut Tool> {
        self.items.iter_mut().rev().find_map(|item| match item {
            Item::Tool(tool) if tool.call_id == call_id => Some(tool),
            _ => None,
        })
    }

    fn notice(&mut self, text: &str) {
        self.items.push(Item::Notice(text.to_owned()));
    }

    /// The answer being written: the last item, or a new one after a tool or a message.
    fn assistant(&mut self) -> (&mut String, &mut String) {
        if !matches!(self.items.last(), Some(Item::Assistant { .. })) {
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

/// A line of what a tool was asked: its command, path or first argument.
fn summary(arguments: &serde_json::Value) -> String {
    let text = ["command", "path", "query", "url", "pattern", "task"]
        .iter()
        .find_map(|key| arguments.get(key).and_then(|value| value.as_str()))
        .map(str::to_owned)
        .unwrap_or_default();
    let line = text.lines().next().unwrap_or_default();
    line.chars().take(160).collect()
}
