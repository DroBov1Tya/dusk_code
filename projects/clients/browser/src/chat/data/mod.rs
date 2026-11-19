//! What one chat shows, built from its history and then from its live events. Plain data
//! with no browser in it, so the drawing code only reads it and the rules can be tested.

use dusk_protocol::{
    catalog::SkillChange,
    config::ApprovalPolicy,
    model::{ModelEvent, ToolCall},
    thread::{
        AgentState, Block, JobResult, LiveTurn, Message, MessageRole, Note, Pin, Task, ThreadEvent,
        ThreadTokens, Tool, ToolState, TurnSpend, TurnWait, Work, WorkKind, activity, agent_name,
        clip, note, turn_spend,
    },
    tools::{Answered, ApprovalRequest, Asked, Card, CardKind, Question},
};
use std::collections::BTreeMap;

#[derive(Clone, PartialEq)]
pub enum Item {
    User(String),
    Answered(Vec<Answered>),
    Job(JobResult),
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
    /// What a `show` call put in the chat, in place of its step.
    Block(Block),
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
    /// The live signals the working row's kind is chosen from.
    pub live: LiveTurn,
    /// Agents below the chat are at work, as the core says; the sidebar shows it working.
    pub agents_working: bool,
    /// When the running turn began, in Unix milliseconds of the core's clock.
    pub turn_started: Option<i64>,
    /// What the running turn waits for, until its next `TurnRunning`.
    pub waiting: Option<TurnWait>,
    /// What the running turn has used so far, as the core counted it.
    pub spend: Option<TurnSpend>,
    /// The context is being compacted.
    pub compacting: bool,
    /// The context used and its limit.
    pub tokens: Option<(u64, u64)>,
    pub cost: Option<String>,
    /// The token report as the core last sent it, for the Overview.
    pub usage: Option<ThreadTokens>,
    /// Counts the turns that ended and the plan's and team's changes, so the open
    /// Overview knows to ask again.
    pub moved: u64,
    pub access: Option<ApprovalPolicy>,
    pub tasks: Vec<Task>,
    /// The pinned messages, as the core cut their excerpts.
    pub pins: Vec<Pin>,
    /// What agents changed in skills, as the core recorded it; a row hangs under the step
    /// whose `call_id` it names.
    pub skill_changes: Vec<SkillChange>,
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
    /// The stored message behind an entry, by entry ID, for forking from it.
    pub message_ids: BTreeMap<u64, String>,
    /// The stream broke or the thread ended; sending opens it again.
    pub ended: bool,
    asked: Asked,
    /// The blocks of `show` calls whose result has not come yet, by call ID.
    showing: BTreeMap<String, Block>,
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
            MessageRole::User => {
                self.add_user(message.content.clone());
                self.message_ids
                    .insert(self.next_id, message.message_id.clone());
            }
            MessageRole::Job => self.push(Item::Job(JobResult::read(&message.content))),
            MessageRole::Assistant => {
                if !message.content.is_empty() || message.reasoning.is_some() {
                    self.push(Item::Assistant {
                        text: message.content.clone(),
                        reasoning: message.reasoning.clone().unwrap_or_default(),
                    });
                    self.message_ids
                        .insert(self.next_id, message.message_id.clone());
                }
                for call in &message.tool_calls {
                    self.add_call(call, ToolState::Done);
                }
            }
            MessageRole::Tool => {
                let id = message.tool_call_id.as_deref().unwrap_or_default();
                if !self.show_block(id, &message.content)
                    && let Some(tool) = self.tool_mut(id)
                {
                    tool.output = clip(&message.content);
                }
            }
        }
        self.add_cards(&message.artifacts);
    }

    fn add_call(&mut self, call: &ToolCall, state: ToolState) {
        self.asked.call(call);
        if let Some(block) = Block::asked(call) {
            self.showing.insert(call.id.clone(), block);
        }
        self.push(Item::Tool(Tool::new(call, state)));
    }

    /// Puts the block of call `id` in place of its step when `result` says it was shown;
    /// a call that was not shown stays a step with its error.
    fn show_block(&mut self, id: &str, result: &str) -> bool {
        let Some(block) = self.showing.remove(id).filter(|b| b.shown_by(result)) else {
            return false;
        };
        let step = self
            .items
            .iter_mut()
            .rev()
            .find(|entry| matches!(&entry.item, Item::Tool(tool) if tool.call_id == id));
        match step {
            Some(entry) => {
                entry.item = Item::Block(block);
                true
            }
            None => false,
        }
    }

    fn add_user(&mut self, text: String) {
        let item = match self.asked.answer(&text) {
            Some(answers) => Item::Answered(answers),
            None => Item::User(text),
        };
        self.push(item);
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

    /// What the working row says about the running turn.
    fn track_turn(&mut self, event: &ThreadEvent) {
        if let Some(spend) = turn_spend(event) {
            self.spend = spend;
        }
        match event {
            ThreadEvent::TurnStarted(turn)
            | ThreadEvent::TurnRunning(turn)
            | ThreadEvent::TurnWaiting(turn) => {
                self.turn_started = Some(turn.started_at);
                self.waiting = turn.waiting;
            }
            ThreadEvent::TurnFinished(_)
            | ThreadEvent::TurnFailed(_)
            | ThreadEvent::Failed(_)
            | ThreadEvent::Idle => {
                self.turn_started = None;
                self.waiting = None;
                self.compacting = false;
            }
            ThreadEvent::CompactionStarted => self.compacting = true,
            ThreadEvent::CompactionFinished(_) | ThreadEvent::CompactionFailed { .. } => {
                self.compacting = false;
            }
            _ => {}
        }
    }

    pub fn apply(&mut self, event: ThreadEvent) {
        self.clock = now_ms();
        self.live.apply(&event, self.clock);
        self.track_turn(&event);
        if matches!(
            event,
            ThreadEvent::TurnFinished(_) | ThreadEvent::TaskChanged(_) | ThreadEvent::Team(_)
        ) {
            self.moved += 1;
        }
        // The row is the whole of a retry: it leaves no notice.
        if matches!(event, ThreadEvent::Retrying { .. }) {
            return;
        }
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
            ThreadEvent::UserMessage(message) => {
                if let Some(at) = self.pending.iter().position(|p| *p == message.content) {
                    self.pending.remove(at);
                }
                self.add_user(message.content);
            }
            ThreadEvent::FromJob(message) => {
                self.push(Item::Job(JobResult::read(&message.content)));
            }
            ThreadEvent::TurnStarted(_) | ThreadEvent::TurnRunning(_) => self.working = true,
            ThreadEvent::TurnFinished(_) | ThreadEvent::Idle => self.working = false,
            ThreadEvent::Model(ModelEvent::Text(text)) => self.assistant().0.push_str(&text),
            ThreadEvent::Model(ModelEvent::Reasoning(text)) => self.assistant().1.push_str(&text),
            ThreadEvent::Model(_) => {}
            ThreadEvent::ToolStarted(call) => self.add_call(&call, ToolState::Running),
            ThreadEvent::ToolFinished { call_id, result } => {
                if !self.show_block(&call_id, &result.content)
                    && let Some(tool) = self.tool_mut(&call_id)
                {
                    tool.finish(&result);
                }
                self.add_cards(&result.artifacts);
            }
            ThreadEvent::Tokens { tokens, limit } => {
                self.tokens = Some((tokens.context, limit));
                self.cost = tokens.cost();
                self.usage = Some(tokens);
            }
            ThreadEvent::ModelSelected { input_limit, .. } => {
                let used = self.tokens.map_or(0, |(used, _)| used);
                self.tokens = Some((used, input_limit));
            }
            ThreadEvent::Tasks(tasks) => self.tasks = tasks,
            ThreadEvent::Pins(pins) => self.pins = pins,
            ThreadEvent::SkillChanged(change) => self.put_skill_change(change),
            ThreadEvent::TaskChanged(task) => {
                match self.tasks.iter_mut().find(|t| t.task_id == task.task_id) {
                    Some(known) => *known = task,
                    None => self.tasks.push(task),
                }
            }
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
            ThreadEvent::AgentsWorking(on) => self.agents_working = on,
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

    /// Keeps `change` in place of the row with its `id`, or as a new one.
    pub fn put_skill_change(&mut self, change: SkillChange) {
        match self.skill_changes.iter_mut().find(|c| c.id == change.id) {
            // An undo is final: a stale list must not offer Undo again.
            Some(known) => {
                let undone = known.undone || change.undone;
                *known = change;
                if undone {
                    known.undone = true;
                    known.can_undo = false;
                }
            }
            None => self.skill_changes.push(change),
        }
    }

    /// The skill changes made by the step `call_id`, oldest first.
    pub fn skill_changes_of(&self, call_id: &str) -> Vec<SkillChange> {
        self.skill_changes
            .iter()
            .filter(|c| c.call_id == call_id)
            .cloned()
            .collect()
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

impl ChatData {
    /// What the chat does at `now`, as the protocol tells the kinds apart; none when
    /// nothing runs. A chat marked as working before its turn's first event thinks.
    pub fn work(&self, now: i64) -> Option<Work> {
        activity(&self.live, now).or_else(|| {
            self.working.then(|| Work {
                kind: WorkKind::Thinking,
                label: "Thinking".to_owned(),
                stalled: false,
            })
        })
    }

    /// The entry whose text grows right now: the answer being written, for the caret
    /// at its end.
    pub fn writing(&self) -> Option<u64> {
        let writes = self
            .work(now_ms())
            .is_some_and(|work| work.kind == WorkKind::Writing);
        match self.items.last() {
            Some(entry) if writes && matches!(entry.item, Item::Assistant { .. }) => Some(entry.id),
            _ => None,
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
            ThreadEvent::Failed(_) | ThreadEvent::TurnFailed(_) => AgentState::Failed,
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
