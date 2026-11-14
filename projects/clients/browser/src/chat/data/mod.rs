//! What one chat shows, built from its history and then from its live events. Plain data
//! with no browser in it, so the drawing code only reads it and the rules can be tested.

use dusk_protocol::{
    config::ApprovalPolicy,
    model::ModelEvent,
    thread::{
        AgentState, Message, MessageRole, Note, Task, ThreadEvent, Tool, ToolState, agent_name,
        clip, note,
    },
    tools::{ApprovalRequest, Card, CardKind, Question},
};
use std::collections::BTreeMap;

#[derive(Clone, PartialEq)]
pub enum Item {
    User(String),
    /// What a subagent reported back.
    Agent {
        from: String,
        text: String,
    },
    Assistant {
        text: String,
        reasoning: String,
    },
    Tool(Tool),
    Image(Shown),
    Audio(Shown),
    Notice(String),
    Error(String),
}

/// A picture or a sound card, by its ID in the artifact folder.
#[derive(Clone, PartialEq)]
pub struct Shown {
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
}

impl From<&Card> for Shown {
    fn from(card: &Card) -> Self {
        Self {
            id: card.id.clone(),
            title: card.title.clone(),
            description: card.description.clone(),
        }
    }
}

/// An item with a number that stays the same while the list changes, for keyed drawing.
#[derive(Clone, PartialEq)]
pub struct Entry {
    pub id: u64,
    pub item: Item,
    /// When it came, in Unix milliseconds, for how long a turn took.
    pub at: i64,
}

/// A subagent of the chat and what it does now.
#[derive(Clone, PartialEq)]
pub struct Agent {
    pub thread_id: String,
    pub name: String,
    pub role: String,
    pub task: String,
    pub state: AgentState,
}

#[derive(Clone, Default, PartialEq)]
pub struct ChatData {
    pub items: Vec<Entry>,
    pub working: bool,
    /// The context used and its limit.
    pub tokens: Option<(u64, u64)>,
    pub cost: Option<String>,
    pub access: Option<ApprovalPolicy>,
    pub tasks: Vec<Task>,
    pub agents: Vec<Agent>,
    pub approvals: Vec<ApprovalRequest>,
    pub question: Option<Question>,
    /// Questions asked while one was on the page, and the answers given so far: they go
    /// to the agent together, after the last one.
    pub questions_waiting: std::collections::VecDeque<Question>,
    pub answers: Vec<String>,
    pub title: Option<String>,
    /// Text the core gave back after /undo, for the composer to offer again.
    pub offered: Option<String>,
    /// What each subagent said and did, by its thread ID.
    pub agent_chats: BTreeMap<String, ChatData>,
    /// Messages this page sent that the core has not taken into the chat yet.
    pub pending: Vec<String>,
    /// The stream broke or the thread ended; sending opens it again.
    pub ended: bool,
    next_id: u64,
    /// The time new items take: a stored message's own, or now for a live event.
    clock: i64,
}

impl ChatData {
    pub fn from_history(history: &[Message]) -> Self {
        let mut chat = Self::default();
        for message in history {
            chat.clock = message.created_at;
            chat.add_message(message);
        }
        chat
    }

    fn push(&mut self, item: Item) {
        self.next_id += 1;
        self.items.push(Entry {
            id: self.next_id,
            item,
            at: self.clock,
        });
    }

    fn add_message(&mut self, message: &Message) {
        match message.role {
            MessageRole::System => {}
            MessageRole::User if message.from_thread_id.is_some() => {
                let from = message.from_thread_id.as_deref().unwrap_or_default();
                let from = self.agent_name(from);
                self.push(Item::Agent {
                    from,
                    text: message.content.clone(),
                });
            }
            MessageRole::User | MessageRole::Job => self.push(Item::User(message.content.clone())),
            MessageRole::Assistant => {
                if !message.content.is_empty() || message.reasoning.is_some() {
                    self.push(Item::Assistant {
                        text: message.content.clone(),
                        reasoning: message.reasoning.clone().unwrap_or_default(),
                    });
                }
                for call in &message.tool_calls {
                    self.push(Item::Tool(Tool::new(call, ToolState::Done)));
                }
            }
            MessageRole::Tool => {
                let id = message.tool_call_id.as_deref().unwrap_or_default();
                if let Some(tool) = self.tool_mut(id) {
                    tool.output = clip(&message.content);
                }
            }
        }
        self.add_cards(&message.artifacts);
    }

    fn add_cards(&mut self, cards: &[Card]) {
        for card in cards {
            match card.kind {
                CardKind::Audio => self.push(Item::Audio(card.into())),
                CardKind::Image => self.push(Item::Image(card.into())),
            }
        }
    }

    /// The page just sent `text`; it shows as queued until the core takes it in.
    pub fn sent(&mut self, text: String) {
        self.pending.push(text);
    }

    pub fn apply(&mut self, event: ThreadEvent) {
        self.clock = now_ms();
        if let Some(note) = note(&event) {
            if matches!(event, ThreadEvent::Failed(_)) {
                self.working = false;
            }
            self.push(match note {
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
                self.push(Item::Agent {
                    from,
                    text: message.content,
                });
            }
            ThreadEvent::UserMessage(message) | ThreadEvent::FromJob(message) => {
                if let Some(at) = self.pending.iter().position(|p| *p == message.content) {
                    self.pending.remove(at);
                }
                self.push(Item::User(message.content));
            }
            ThreadEvent::TurnStarted(_) => self.working = true,
            ThreadEvent::TurnFinished(_) | ThreadEvent::Idle => self.working = false,
            ThreadEvent::Model(ModelEvent::Text(text)) => self.assistant().0.push_str(&text),
            ThreadEvent::Model(ModelEvent::Reasoning(text)) => self.assistant().1.push_str(&text),
            ThreadEvent::Model(_) => {}
            ThreadEvent::ToolStarted(call) => {
                self.push(Item::Tool(Tool::new(&call, ToolState::Running)))
            }
            ThreadEvent::ToolFinished { call_id, result } => {
                if let Some(tool) = self.tool_mut(&call_id) {
                    tool.finish(&result);
                }
                self.add_cards(&result.artifacts);
            }
            ThreadEvent::Tokens { tokens, limit } => {
                self.tokens = Some((tokens.context, limit));
                self.cost = tokens.cost();
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
            } => self.agents.push(Agent {
                name: agent_name(&thread_id),
                thread_id,
                role: agent,
                task,
                state: AgentState::Thinking,
            }),
            ThreadEvent::Subagent { thread_id, event } => {
                if let Some(agent) = self.agents.iter_mut().find(|a| a.thread_id == thread_id) {
                    agent.follow(&event);
                }
                self.agent_chats.entry(thread_id).or_default().apply(*event);
            }
            ThreadEvent::Question(question) if self.question.is_some() => {
                self.questions_waiting.push_back(question)
            }
            ThreadEvent::Question(question) => self.question = Some(question),
            ThreadEvent::Approval(request) => self.approvals.push(request),
            ThreadEvent::ApprovalClosed { id } => self.approvals.retain(|r| r.id != id),
            ThreadEvent::TitleChanged { title } => self.title = Some(title),
            ThreadEvent::Undone { text } => self.offered = Some(text),
            // The rest left a note above or is not shown.
            _ => {}
        }
    }

    fn agent_name(&self, thread_id: &str) -> String {
        self.agents
            .iter()
            .find(|agent| agent.thread_id == thread_id)
            .map_or_else(|| "an agent".to_owned(), |agent| agent.name.clone())
    }

    fn tool_mut(&mut self, call_id: &str) -> Option<&mut Tool> {
        self.items
            .iter_mut()
            .rev()
            .find_map(|entry| match &mut entry.item {
                Item::Tool(tool) if tool.call_id == call_id => Some(tool),
                _ => None,
            })
    }

    /// The answer being written: the last item, or a new one after a tool or a message.
    fn assistant(&mut self) -> (&mut String, &mut String) {
        if !matches!(
            self.items.last(),
            Some(Entry {
                item: Item::Assistant { .. },
                ..
            })
        ) {
            self.push(Item::Assistant {
                text: String::new(),
                reasoning: String::new(),
            });
        }
        match self.items.last_mut() {
            Some(Entry {
                item: Item::Assistant { text, reasoning },
                ..
            }) => (text, reasoning),
            _ => unreachable!("an answer was just pushed"),
        }
    }
}

impl Agent {
    fn follow(&mut self, event: &ThreadEvent) {
        self.state = match event {
            ThreadEvent::TurnStarted(_) | ThreadEvent::ToolFinished { .. } => AgentState::Thinking,
            ThreadEvent::Model(ModelEvent::Reasoning(_)) => AgentState::Thinking,
            ThreadEvent::Model(ModelEvent::Text(_)) => AgentState::Writing,
            ThreadEvent::ToolStarted(call) => AgentState::Tool(call.name.clone()),
            ThreadEvent::Idle => AgentState::Done,
            ThreadEvent::Failed(_) => AgentState::Failed,
            _ => return,
        };
    }
}

#[cfg(test)]
mod tests;

/// The browser's clock in Unix milliseconds; tests on the host read zero.
pub fn now_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    return js_sys::Date::now() as i64;
    #[cfg(not(target_arch = "wasm32"))]
    0
}
