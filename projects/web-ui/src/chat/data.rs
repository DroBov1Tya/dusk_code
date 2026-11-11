//! What one chat shows, built from its history and then from its live events. Plain data
//! with no browser in it, so the drawing code only reads it and the rules can be tested.

use dusk_protocol::{
    config::ApprovalPolicy,
    model::ModelEvent,
    thread::{Message, MessageRole, Task, ThreadEvent, agent_name},
    tools::{ApprovalRequest, Card, CardKind, Question},
};
use std::collections::BTreeMap;

#[derive(Clone, PartialEq)]
pub enum Item {
    User(String),
    /// What a subagent reported back.
    Agent { from: String, text: String },
    Assistant { text: String, reasoning: String },
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

#[derive(Clone, PartialEq)]
pub struct Tool {
    pub call_id: String,
    pub name: String,
    /// The first line of what it was asked: its command, path or query.
    pub detail: String,
    pub arguments: String,
    pub output: String,
    pub state: ToolState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Running,
    Done,
    Failed,
}

/// How much of a tool's arguments and answer an unfolded step shows.
const SHOWN_CHARS: usize = 4000;

/// An item with a number that stays the same while the list changes, for keyed drawing.
#[derive(Clone, PartialEq)]
pub struct Entry {
    pub id: u64,
    pub item: Item,
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

#[derive(Clone, PartialEq, Eq)]
pub enum AgentState {
    Thinking,
    Writing,
    Tool(String),
    Done,
    Failed,
}

impl AgentState {
    pub fn working(&self) -> bool {
        !matches!(self, Self::Done | Self::Failed)
    }
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
}

impl ChatData {
    pub fn from_history(history: &[Message]) -> Self {
        let mut chat = Self::default();
        for message in history {
            chat.add_message(message);
        }
        chat
    }

    fn push(&mut self, item: Item) {
        self.next_id += 1;
        self.items.push(Entry {
            id: self.next_id,
            item,
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
                    self.push(Item::Tool(Tool {
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
            ThreadEvent::ToolStarted(call) => self.push(Item::Tool(Tool {
                detail: summary(&call.arguments),
                arguments: clip(&call.arguments.to_string()),
                output: String::new(),
                call_id: call.id,
                name: call.name,
                state: ToolState::Running,
            })),
            ThreadEvent::ToolFinished { call_id, result } => {
                if let Some(tool) = self.tool_mut(&call_id) {
                    tool.state = match result.is_error {
                        true => ToolState::Failed,
                        false => ToolState::Done,
                    };
                    tool.output = clip(&result.content);
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
            ThreadEvent::Question(question) => self.question = Some(question),
            ThreadEvent::Approval(request) => self.approvals.push(request),
            ThreadEvent::ApprovalClosed { id } => self.approvals.retain(|r| r.id != id),
            ThreadEvent::TitleChanged { title } => self.title = Some(title),
            ThreadEvent::Undone { text } => self.offered = Some(text),
            ThreadEvent::Failed(error) => {
                self.working = false;
                self.push(Item::Error(error));
            }
            ThreadEvent::Rejected { reason } => self.push(Item::Error(reason)),
            ThreadEvent::ProviderUnavailable => self.push(Item::Error(
                "The provider is unavailable; check the connection in the settings.".into(),
            )),
            ThreadEvent::Retrying { attempt, error } => {
                self.notice(&format!("Retrying ({attempt}): {error}"));
            }
            ThreadEvent::AnswerCut { error } => self.push(Item::Error(error)),
            ThreadEvent::CompactionFailed { error } => {
                self.push(Item::Error(format!("Compaction failed: {error}")));
            }
            ThreadEvent::OutputLimitReached => self.notice("The answer reached the output limit."),
            ThreadEvent::PicturesRefused { model } => {
                self.notice(&format!("{model} does not take pictures."));
            }
            ThreadEvent::CompactionStarted => self.notice("Compacting the context..."),
            ThreadEvent::CompactionFinished(_) => self.notice("The context was compacted."),
            ThreadEvent::ContextTooLarge { tokens, limit } => self.push(Item::Error(format!(
                "The context is too large: {tokens} of {limit} tokens."
            ))),
            ThreadEvent::ArbiterDecided {
                what,
                allow,
                reason,
            } => {
                let verdict = if allow { "allowed" } else { "denied" };
                self.notice(&format!("Arbiter {verdict} {what}: {reason}"));
            }
            ThreadEvent::Goal { note, .. } => self.notice(&note),
            ThreadEvent::Forked { .. } => self.notice("Forked into a new chat."),
            ThreadEvent::AgentPhase { .. }
            | ThreadEvent::LocalUnsent(_)
            | ThreadEvent::Queued { .. }
            | ThreadEvent::OutputRate { .. } => {}
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

    fn notice(&mut self, text: &str) {
        self.push(Item::Notice(text.to_owned()));
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

fn clip(text: &str) -> String {
    let mut clipped: String = text.chars().take(SHOWN_CHARS).collect();
    if clipped.len() < text.len() {
        clipped.push_str("\n...");
    }
    clipped
}

/// A line of what a tool was asked: its command, path or first argument.
fn summary(arguments: &serde_json::Value) -> String {
    let text = [
        "description",
        "command",
        "path",
        "query",
        "url",
        "pattern",
        "task",
    ]
    .iter()
    .find_map(|key| arguments.get(key).and_then(|value| value.as_str()))
    .unwrap_or_default();
    let line = text.lines().next().unwrap_or_default();
    line.chars().take(160).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::model::ToolCall;
    use dusk_protocol::tools::ToolResult;

    fn call(id: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: "bash".into(),
            arguments: serde_json::json!({ "command": "ls\nmore" }),
        }
    }

    #[test]
    fn streamed_text_joins_one_answer_and_a_tool_splits_it() {
        let mut chat = ChatData::default();
        chat.apply(ThreadEvent::Model(ModelEvent::Text("Hel".into())));
        chat.apply(ThreadEvent::Model(ModelEvent::Text("lo".into())));
        chat.apply(ThreadEvent::ToolStarted(call("1")));
        chat.apply(ThreadEvent::Model(ModelEvent::Text("Done".into())));
        let kinds: Vec<&str> = chat
            .items
            .iter()
            .map(|entry| match &entry.item {
                Item::Assistant { .. } => "answer",
                Item::Tool(_) => "tool",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["answer", "tool", "answer"]);
        assert!(matches!(&chat.items[0].item, Item::Assistant { text, .. } if text == "Hello"));
    }

    #[test]
    fn a_tool_result_lands_on_its_call() {
        let mut chat = ChatData::default();
        chat.apply(ThreadEvent::ToolStarted(call("1")));
        chat.apply(ThreadEvent::ToolFinished {
            call_id: "1".into(),
            result: ToolResult::text("out".into()),
        });
        let Item::Tool(tool) = &chat.items[0].item else {
            panic!("no tool");
        };
        assert_eq!((tool.state, tool.output.as_str()), (ToolState::Done, "out"));
        assert_eq!(tool.detail, "ls");
    }

    #[test]
    fn an_approval_shows_until_it_is_closed() {
        let mut chat = ChatData::default();
        let request = ApprovalRequest {
            id: 4,
            request: dusk_protocol::tools::Request::Command {
                command: "rm x".into(),
            },
            session: None,
        };
        chat.apply(ThreadEvent::Approval(request));
        assert_eq!(chat.approvals.len(), 1);
        chat.apply(ThreadEvent::ApprovalClosed { id: 4 });
        assert!(chat.approvals.is_empty());
    }

    #[test]
    fn a_subagents_events_go_to_its_own_chat() {
        let mut chat = ChatData::default();
        chat.apply(ThreadEvent::SubagentStarted {
            call_id: "c".into(),
            thread_id: "sub".into(),
            agent: "general".into(),
            task: "look".into(),
            effort: None,
        });
        chat.apply(ThreadEvent::Subagent {
            thread_id: "sub".into(),
            event: Box::new(ThreadEvent::Model(ModelEvent::Text("hi".into()))),
        });
        assert!(chat.items.is_empty());
        assert_eq!(chat.agent_chats["sub"].items.len(), 1);
        assert!(matches!(chat.agents[0].state, AgentState::Writing));
    }
}
