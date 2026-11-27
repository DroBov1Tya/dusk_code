//! One chat as every client shows it: the items built from the stored history and then
//! from live events, with the state around them (the running turn, subagents, tasks,
//! tokens, skill changes). The clients draw it; the rules of what an event does to a
//! chat live here once.
//!
//! An item keeps an ID that stays while the list changes, the time it came, the stored
//! message behind it and a revision that changes whenever the item does, so a client
//! redraws or measures only what changed. `shape` changes whenever the list does in a way
//! that moves rows: an item came, went or became another kind, an answer's text or
//! thinking stopped being blank, or a turn boundary was drawn.

mod history;
mod run;
mod team;

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{AgentState, Note, Run, Tool, ToolState, TurnSpend};
use super::{note, turn_spend};
use crate::{
    catalog::SkillChange,
    model::{Effort, ModelEvent, ToolCall},
    plugin::{SUGGEST_TOOL, Suggestion},
    thread::{
        Block, GoalEvaluation, JobResult, LiveTurn, PROPOSE_SESSION_TOOL, Pin, Proposal,
        ProposalCalls, ProposalState, SUGGEST_NEXT_TOOL, Task, Team, ThreadEvent, ThreadTokens,
        TurnStatus, TurnWait, Work, WorkKind, activity, agent_name, read_outcome,
    },
    tools::{Answered, ApprovalRequest, Asked, Card, CardKind},
};

/// What the result of a call that the turn's end cut short says, as the core stores it.
pub const UNFINISHED: &str = "no result: the turn ended before this call finished";
/// What the core stores for a call the user's stop cut short.
pub const INTERRUPTED: &str = "stopped: the user interrupted the turn";

/// One thing a chat shows. `L` is what a client adds of its own, such as a command the
/// user ran from an answer; the core never sends one.
#[derive(Debug, Clone, PartialEq)]
pub enum Item<L = Infallible> {
    /// What the user wrote. `sequence` is the stored message's place, the point /undo goes
    /// back to; a message echoed by the client before the core stored it has none.
    /// `answered` holds the agent's questions it answers, drawn in place of its text.
    User {
        text: String,
        sequence: Option<u64>,
        answered: Vec<Answered>,
    },
    Job(JobResult),
    /// What a subagent reported back: the agent's side of the chat, not the user's.
    /// `from` is its name, "an agent" when it is not known.
    Agent {
        from: String,
        thread_id: Option<String>,
        text: String,
    },
    Assistant {
        text: String,
        reasoning: String,
    },
    /// What a compaction wrote while it summarized the older messages.
    Summary(String),
    Tool(Tool),
    /// What a `show` call put in the chat, in place of that call's step.
    Block(Block),
    /// The Workshop listings a `suggest_from_workshop` call put in the chat, in place of
    /// that call's step.
    Suggestion(Suggestion),
    /// A session the agent proposed with `propose_session`, in place of that call's step,
    /// and where it stands: the user starts or dismisses it from the card, which names
    /// it by `proposal.id`.
    Proposal {
        proposal: Proposal,
        state: ProposalState,
    },
    /// A team or subagents the chat started, in place of the steps that started them;
    /// `ChatLog::run_card` says what its card shows.
    Run(Run),
    /// A picture a tool or a message carries. `aside`: a picture the model took to see,
    /// such as a screenshot, which a client may keep beside the chat instead of in it;
    /// those `display_image` shows and those of a message are not aside.
    Image {
        card: Card,
        call_id: Option<String>,
        aside: bool,
    },
    Audio(Card),
    /// What the evaluator said of the chat's goal after a turn.
    Goal(GoalEvaluation),
    Notice(String),
    Error(String),
    Local(L),
}

impl<L> Item<L> {
    /// The answer's text and thinking are both blank, as a pair: what tells whether it is
    /// drawn, so a change of either moves rows.
    fn blanks(&self) -> Option<(bool, bool)> {
        match self {
            Item::Assistant { text, reasoning } => {
                Some((text.trim().is_empty(), reasoning.trim().is_empty()))
            }
            _ => None,
        }
    }
}

/// What a chat knows of an item beside the item itself.
#[derive(Debug, Clone, PartialEq)]
struct Meta {
    id: u64,
    at: i64,
    message_id: Option<String>,
    revision: u64,
}

/// A subagent of the chat and what it does now.
#[derive(Debug, Clone, PartialEq)]
pub struct Subagent {
    pub thread_id: String,
    /// The name drawn from its thread, such as `Ember-0c`.
    pub name: String,
    /// The agent it works as, such as `general`.
    pub role: String,
    /// The model it answers on, `provider/model`; none when the core did not say.
    pub model: Option<String>,
    /// How hard it reasons; none is the provider's default, and what a team's own event
    /// does not say.
    pub effort: Option<Effort>,
    /// The `ask_agent` call that started it, or `team:<task_id>` for a team member.
    pub call_id: String,
    pub task: String,
    /// The task of the plan it works on, when it is a team member.
    pub task_id: Option<String>,
    pub state: AgentState,
    /// The tools it runs now, by call ID and name, oldest first.
    pub running: Vec<(String, String)>,
    /// When it started and ended, in Unix milliseconds of the client's clock.
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

impl Subagent {
    pub fn working(&self) -> bool {
        self.state.working()
    }

    /// `general · chatgpt/gpt-5.5 · high effort`: what it works as, on which model and how
    /// hard it reasons, each part only when known.
    pub fn role_and_model(&self) -> String {
        let effort = self
            .effort
            .map(|effort| format!("{} effort", effort.name()));
        [Some(self.role.clone()), self.model.clone(), effort]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" \u{00B7} ")
    }

    /// How long it worked, until `now` while it still does.
    pub fn run_ms(&self, now: i64) -> i64 {
        self.ended_at
            .unwrap_or(now)
            .saturating_sub(self.started_at)
            .max(0)
    }

    /// The user stopped it.
    pub fn stop(&mut self, now: i64) {
        self.state = AgentState::Done;
        self.running.clear();
        self.ended_at = self.ended_at.or(Some(now));
    }

    /// Follows an event of its thread; whether what it does changed.
    pub fn follow(&mut self, event: &ThreadEvent, now: i64) -> bool {
        let changed = self.state.follow(&mut self.running, event);
        self.ended_at = match self.working() {
            true => None,
            false => self.ended_at.or(Some(now)),
        };
        changed
    }
}

/// What one event changed, so a client can redraw no more than that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// Only the content of the item at this index changed, as streamed text does; its
    /// blankness did not, so no row moved.
    Item(usize),
    /// Only what the subagent of this thread did changed: its own chat, not this one's
    /// items or state.
    Subagent(String),
    /// Anything may have changed.
    Any,
}

pub struct ChatLog<L = Infallible> {
    pub items: Vec<Item<L>>,
    meta: Vec<Meta>,
    next_id: u64,
    revisions: u64,
    shape: u64,
    changed_from: Option<usize>,
    /// Where turns part, as item indexes, oldest first: a turn's start draws one after
    /// the message that opened it, its end one after its last item; never two at one
    /// place. A chat read from history draws the same ones.
    pub activity_boundaries: Vec<usize>,
    /// When each boundary was drawn, in Unix milliseconds, beside `activity_boundaries`.
    pub boundary_times: Vec<i64>,
    /// What the turn ending at each boundary spent, as tokens and micro-dollars, as the
    /// core said when it finished; none for a turn read from history.
    pub boundary_spend: Vec<Option<(u64, u64)>>,
    pub working: bool,
    /// The live signals the kind of work is chosen from (`work`). Every event the chat
    /// takes feeds it, so no client does.
    pub live: LiveTurn,
    /// Agents below the chat are at work, as the core says.
    pub agents_working: bool,
    /// The context is being compacted.
    pub compacting: bool,
    /// When the running turn began, in Unix milliseconds of the core's clock.
    pub turn_started: Option<i64>,
    /// What the running turn waits for, as the core last said.
    pub waiting: Option<TurnWait>,
    /// What the running turn has used so far, as the core counted it.
    pub spend: Option<TurnSpend>,
    /// The context used and its limit.
    pub tokens: Option<(u64, u64)>,
    /// The token report as the core last sent it.
    pub usage: Option<ThreadTokens>,
    /// What the chat cost so far, when any call's cost is known.
    pub cost: Option<String>,
    pub tasks: Vec<Task>,
    /// The team running the chat's plan, as the core last told it.
    pub team: Option<Team>,
    pub pins: Vec<Pin>,
    /// What agents changed in skills, oldest first; a change hangs under the step whose
    /// `call_id` it names.
    pub skill_changes: Vec<SkillChange>,
    /// The subagents, by thread ID.
    pub agents: BTreeMap<String, Subagent>,
    /// What each subagent said and did, by its thread ID.
    pub agent_chats: BTreeMap<String, ChatLog<L>>,
    pub approvals: Vec<ApprovalRequest>,
    pub title: Option<String>,
    /// Text the core gave back after /undo, for the composer to offer again.
    pub offered: Option<String>,
    /// Where the items are cut once the core confirms an /undo.
    pub rollback_at: Option<usize>,
    asked: Asked,
    /// The blocks `show` calls ask for, by call, until their results say whether they
    /// were shown.
    showing: BTreeMap<String, Block>,
    /// The `propose_session` calls whose result may put a card in place of their step.
    proposing: ProposalCalls,
    /// The subagents' run of the step under way, which its next `ask_agent` joins.
    batch: Option<usize>,
}

impl<L> Default for ChatLog<L> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            meta: Vec::new(),
            next_id: 0,
            revisions: 0,
            shape: 0,
            changed_from: None,
            activity_boundaries: Vec::new(),
            boundary_times: Vec::new(),
            boundary_spend: Vec::new(),
            working: false,
            live: LiveTurn::default(),
            agents_working: false,
            compacting: false,
            turn_started: None,
            waiting: None,
            spend: None,
            tokens: None,
            usage: None,
            cost: None,
            tasks: Vec::new(),
            team: None,
            pins: Vec::new(),
            skill_changes: Vec::new(),
            agents: BTreeMap::new(),
            agent_chats: BTreeMap::new(),
            approvals: Vec::new(),
            title: None,
            offered: None,
            rollback_at: None,
            asked: Asked::default(),
            showing: BTreeMap::new(),
            proposing: ProposalCalls::default(),
            batch: None,
        }
    }
}

/// The item list and what a client reads of each item.
impl<L> ChatLog<L> {
    /// The ID of the item at `index`, which stays while the list changes.
    pub fn id(&self, index: usize) -> u64 {
        self.meta.get(index).map_or(0, |meta| meta.id)
    }

    /// Where the item with ID `id` is now.
    pub fn index_of(&self, id: u64) -> Option<usize> {
        // IDs only grow along the list: items are pushed at its end and never moved.
        self.meta.binary_search_by_key(&id, |meta| meta.id).ok()
    }

    /// When the item at `index` came, in Unix milliseconds: a stored message's own time,
    /// else the client's clock when its event came.
    pub fn at(&self, index: usize) -> Option<i64> {
        self.meta.get(index).map(|meta| meta.at)
    }

    /// The stored message the item at `index` shows, for pinning, forking and finding it.
    pub fn message_id(&self, index: usize) -> Option<&str> {
        self.meta.get(index)?.message_id.as_deref()
    }

    /// The item that shows the stored message `message_id`.
    pub fn item_of(&self, message_id: &str) -> Option<usize> {
        self.meta
            .iter()
            .position(|meta| meta.message_id.as_deref() == Some(message_id))
    }

    /// Whether the message of the item at `index` is pinned.
    pub fn pinned(&self, index: usize) -> bool {
        self.message_id(index)
            .is_some_and(|id| self.pins.iter().any(|pin| pin.message_id == id))
    }

    /// Changes whenever the item at `index` does.
    pub fn revision(&self, index: usize) -> u64 {
        self.meta.get(index).map_or(0, |meta| meta.revision)
    }

    /// Changes whenever rows may move; see the module's words. Never the same for two
    /// chats, so a client may key what it built from the rows by it alone.
    pub fn shape(&self) -> u64 {
        self.shape
    }

    /// The earliest existing item that changed in place since the last call: its rows
    /// need drawing or measuring again. Items that came are new rows, not in it.
    pub fn take_changed(&mut self) -> Option<usize> {
        self.changed_from.take()
    }

    /// Adds an item at the end, at `at` in Unix milliseconds; its index.
    pub fn push(&mut self, item: Item<L>, at: i64) -> usize {
        self.next_id += 1;
        self.revisions += 1;
        self.reshape();
        self.items.push(item);
        self.meta.push(Meta {
            id: self.next_id,
            at,
            message_id: None,
            revision: self.revisions,
        });
        self.items.len() - 1
    }

    /// Adds items at the end, all at `at`, as a client's demo or test chat does.
    pub fn extend(&mut self, items: impl IntoIterator<Item = Item<L>>, at: i64) {
        for item in items {
            self.push(item, at);
        }
    }

    /// Names the stored message the item at `index` shows.
    pub fn set_message_id(&mut self, index: usize, message_id: String) {
        if let Some(meta) = self.meta.get_mut(index) {
            meta.message_id = Some(message_id);
            self.touch(index);
        }
    }

    /// A line of the client's own, such as what a command it ran said.
    pub fn notice(&mut self, text: impl Into<String>, now: i64) -> usize {
        self.push(Item::Notice(text.into()), now)
    }

    /// The user's own message as the client shows it before the core stores it; one that
    /// answers the agent's question keeps the question beside it.
    pub fn add_user(&mut self, text: String, sequence: Option<u64>, at: i64) -> usize {
        let answered = self.asked.answer(&text).unwrap_or_default();
        self.push(
            Item::User {
                text,
                sequence,
                answered,
            },
            at,
        )
    }

    /// Puts the items of `later`, a chat followed live while this one was read from
    /// history, after this one's; whether a turn runs is as `later` says.
    pub fn append_live(&mut self, later: ChatLog<L>) {
        for (item, meta) in later.items.into_iter().zip(later.meta) {
            let index = self.push(item, meta.at);
            self.meta[index].message_id = meta.message_id;
        }
        self.working = later.working;
    }

    /// The item at `index` to change in place; it is marked changed.
    pub fn item_mut(&mut self, index: usize) -> Option<&mut Item<L>> {
        let blanks = self.items.get(index)?.blanks();
        self.touch(index);
        // A caller that makes an answer blank or not moves rows; a client item never does.
        if blanks.is_some() {
            self.reshape();
        }
        self.items.get_mut(index)
    }

    /// Drops the item at `index`; the boundaries after it move up with it.
    pub fn remove(&mut self, index: usize) -> Option<Item<L>> {
        if index >= self.items.len() {
            return None;
        }
        self.meta.remove(index);
        let item = self.items.remove(index);
        self.batch = None;
        for boundary in &mut self.activity_boundaries {
            if *boundary > index {
                *boundary -= 1;
            }
        }
        self.reshape();
        self.mark(index);
        Some(item)
    }

    /// Marks the item at `index` changed in place.
    fn touch(&mut self, index: usize) {
        self.revisions += 1;
        if let Some(meta) = self.meta.get_mut(index) {
            meta.revision = self.revisions;
        }
        self.mark(index);
    }

    /// A new shape, unique across chats, so a chat read again never looks unchanged.
    fn reshape(&mut self) {
        static SHAPES: AtomicU64 = AtomicU64::new(1);
        self.shape = SHAPES.fetch_add(1, Ordering::Relaxed);
    }

    fn mark(&mut self, index: usize) {
        self.changed_from = Some(self.changed_from.map_or(index, |old| old.min(index)));
    }

    fn tool_index(&self, call_id: &str) -> Option<usize> {
        self.items
            .iter()
            .rposition(|item| matches!(item, Item::Tool(tool) if tool.call_id == call_id))
    }

    /// The words of the last message the user sent, as they went to the core.
    pub fn last_sent(&self) -> Option<&str> {
        self.items.iter().rev().find_map(|item| match item {
            Item::User {
                text,
                sequence: Some(_),
                ..
            } => Some(text.as_str()),
            _ => None,
        })
    }

    /// The pictures kept beside the chat, oldest first.
    pub fn screenshots(&self) -> impl Iterator<Item = &Card> {
        self.items.iter().filter_map(|item| match item {
            Item::Image {
                card, aside: true, ..
            } => Some(card),
            _ => None,
        })
    }

    /// The subagent of thread `thread_id`.
    pub fn agent(&self, thread_id: &str) -> Option<&Subagent> {
        self.agents.get(thread_id)
    }

    pub fn agent_mut(&mut self, thread_id: &str) -> Option<&mut Subagent> {
        self.agents.get_mut(thread_id)
    }

    /// The name of the subagent of thread `thread_id`, or "an agent".
    pub fn agent_name(&self, thread_id: &str) -> String {
        // The name comes from the thread, so a chat read from history, which knows no
        // agents, names an agent's report as a live one does.
        self.agent(thread_id)
            .map_or_else(|| agent_name(thread_id), |agent| agent.name.clone())
    }

    /// The skill changes made by the step `call_id`, oldest first.
    pub fn skill_changes_of<'a>(
        &'a self,
        call_id: &'a str,
    ) -> impl Iterator<Item = &'a SkillChange> {
        self.skill_changes
            .iter()
            .filter(move |change| change.call_id == call_id)
    }

    /// What the chat does at `now` (Unix milliseconds), as `thread::activity` tells the
    /// kinds apart, for the one indicator a client shows while it works; none when
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

    /// The item whose text grows right now: the answer being written, for a client that
    /// marks its end.
    pub fn writing(&self, now: i64) -> Option<usize> {
        let writes = self
            .work(now)
            .is_some_and(|work| work.kind == WorkKind::Writing);
        self.answer_index().filter(|_| writes)
    }

    /// The mean cost of the turns that have one, to tell an unusually dear turn.
    pub fn mean_turn_cost(&self) -> Option<u64> {
        let costs: Vec<u64> = self
            .boundary_spend
            .iter()
            .flatten()
            .map(|(_, micro)| *micro)
            .filter(|micro| *micro > 0)
            .collect();
        (!costs.is_empty()).then(|| costs.iter().sum::<u64>() / costs.len() as u64)
    }

    /// When the turn whose items end at boundary `index` began and ended, in Unix
    /// milliseconds; the turn after the last boundary is still open.
    pub fn span(&self, index: usize) -> Option<(i64, Option<i64>)> {
        let began = *self.boundary_times.get(index.checked_sub(1)?)?;
        Some((began, self.boundary_times.get(index).copied()))
    }
}

/// What the events do.
impl<L> ChatLog<L> {
    /// Folds one live event into the chat, at `now` in Unix milliseconds of the client's
    /// clock.
    pub fn apply(&mut self, event: ThreadEvent, now: i64) -> Applied {
        // The piece that answers a call tried again changes what the indicator says.
        let retried = self.live.retrying();
        self.live.apply(&event, now);
        // A model call opens a new step: its `ask_agent` calls are a run of their own.
        if matches!(event, ThreadEvent::Model(_) | ThreadEvent::TurnStarted(_)) {
            self.batch = None;
        }
        // The hot path: a streamed piece of the answer that moves no row.
        if let ThreadEvent::Model(ModelEvent::Text(piece) | ModelEvent::Reasoning(piece)) = &event
            && !retried
            && !self.compacting
            && let Some(index) = self.answer_index()
        {
            let before = self.items[index].blanks();
            if let Item::Assistant { text, reasoning } = &mut self.items[index] {
                match &event {
                    ThreadEvent::Model(ModelEvent::Text(_)) => text.push_str(piece),
                    _ => reasoning.push_str(piece),
                }
            }
            self.touch(index);
            if self.items[index].blanks() == before {
                return Applied::Item(index);
            }
            self.reshape();
            return Applied::Any;
        }
        if let ThreadEvent::Subagent { thread_id, event } = &event
            && self.agent(thread_id).is_some()
        {
            let changed = self
                .agent_mut(thread_id)
                .is_some_and(|agent| agent.follow(event, now));
            if changed && let Some(call_id) = self.agent(thread_id).map(|a| a.call_id.clone()) {
                self.touch_agent_run(&call_id);
            }
            let chat = self.agent_chats.entry(thread_id.clone()).or_default();
            chat.apply((**event).clone(), now);
            return match changed {
                true => Applied::Any,
                false => Applied::Subagent(thread_id.clone()),
            };
        }
        self.apply_any(event, now);
        Applied::Any
    }

    fn apply_any(&mut self, event: ThreadEvent, now: i64) {
        if let Some(spend) = turn_spend(&event) {
            self.spend = spend;
        }
        match &event {
            ThreadEvent::TurnStarted(turn)
            | ThreadEvent::TurnRunning(turn)
            | ThreadEvent::TurnWaiting(turn) => {
                self.turn_started = Some(turn.started_at);
                self.waiting = turn.waiting;
            }
            ThreadEvent::CompactionStarted => self.compacting = true,
            ThreadEvent::CompactionFinished(_) | ThreadEvent::CompactionFailed { .. } => {
                self.compacting = false
            }
            ThreadEvent::TurnFailed(_) => self.end_turn(),
            _ => {}
        }
        // The indicator is the whole of a retry (`work`): it leaves no notice.
        if matches!(event, ThreadEvent::Retrying { .. }) {
            return;
        }
        if let Some(note) = note(&event) {
            if let ThreadEvent::Failed(_) = event {
                self.working = false;
                self.end_turn();
                self.stop_running_tools(UNFINISHED);
            }
            let item = match note {
                Note::Notice(text) => Item::Notice(text),
                Note::Error(text) => Item::Error(text),
            };
            self.push(item, now);
            return;
        }
        match event {
            ThreadEvent::FromAgent(message) if self.settled_by(&message.content) => {}
            ThreadEvent::FromAgent(message) => {
                let from = message
                    .from_thread_id
                    .as_deref()
                    .map_or_else(|| "an agent".to_owned(), |id| self.agent_name(id));
                self.push(
                    Item::Agent {
                        from,
                        thread_id: message.from_thread_id,
                        text: message.content,
                    },
                    now,
                );
            }
            ThreadEvent::UserMessage(message) | ThreadEvent::FromJob(message) => {
                self.add_message(&message, now);
            }
            ThreadEvent::GoalEvaluated(evaluation) => self.goal_verdict(evaluation, now),
            ThreadEvent::TurnStarted(_) => {
                self.boundary(now);
                self.working = true;
            }
            // The turn began before this client listened: it works, but its items are
            // already in the history and must not open a second boundary. It also comes
            // after a wait, with the turn no longer waiting.
            ThreadEvent::TurnRunning(_) => self.working = true,
            ThreadEvent::TurnFinished(turn) => {
                // The answer streamed without its ID; the core names it as the turn ends.
                if let Some(id) = turn.response_message_id.clone()
                    && let Some(at) = self
                        .items
                        .iter()
                        .rposition(|item| matches!(item, Item::Assistant { .. }))
                    && self.meta[at].message_id.is_none()
                {
                    self.meta[at].message_id = Some(id);
                    self.touch(at);
                }
                let spent = (turn.tokens > 0).then_some((turn.tokens, turn.cost_micro_usd));
                if self.boundary(now)
                    && let Some(last) = self.boundary_spend.last_mut()
                {
                    *last = spent;
                }
                self.working = false;
                self.end_turn();
                // A call the turn's end cut short stopped with it, said as the core stores it.
                let why = match turn.status {
                    TurnStatus::Interrupted => INTERRUPTED,
                    _ => UNFINISHED,
                };
                self.stop_running_tools(why);
            }
            ThreadEvent::Idle => {
                self.boundary(now);
                self.working = false;
                self.end_turn();
            }
            ThreadEvent::Model(ModelEvent::Text(text)) if self.compacting => {
                self.summary(&text, now)
            }
            ThreadEvent::Model(ModelEvent::Text(text)) => {
                let index = self.assistant(now);
                self.stream(index, &text, true);
            }
            ThreadEvent::Model(ModelEvent::Reasoning(text)) => {
                let index = self.assistant(now);
                self.stream(index, &text, false);
            }
            ThreadEvent::Model(_) => {}
            ThreadEvent::ToolStarted(call) => self.start_tool(&call, ToolState::Running, now),
            ThreadEvent::ToolFinished { call_id, result } => {
                let shown = !result.is_error
                    && (self.suggestion(&call_id, &result.content)
                        || self.proposal(&call_id, &result.content));
                if !shown
                    && !self.finish_run(&call_id, &result)
                    && !self.show_block(&call_id, &result.content)
                    && let Some(index) = self.tool_index(&call_id)
                {
                    if let Item::Tool(tool) = &mut self.items[index] {
                        tool.finish(&result);
                    }
                    self.touch(index);
                }
                self.add_pictures(&call_id, &result.artifacts, now);
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
            ThreadEvent::Tasks(tasks) => {
                self.tasks = tasks;
                self.touch_team_run();
            }
            ThreadEvent::TaskChanged(task) => {
                self.touch_team_run();
                match self
                    .tasks
                    .iter_mut()
                    .find(|known| known.task_id == task.task_id)
                {
                    Some(known) => *known = task,
                    None => self.tasks.push(task),
                }
            }
            ThreadEvent::SubagentStarted {
                call_id,
                thread_id,
                agent,
                task,
                effort,
                model,
                task_id,
            } => {
                self.touch_agent_run(&call_id);
                // Asking the same thread again brings it back to work.
                let agent = Subagent {
                    name: agent_name(&thread_id),
                    thread_id: thread_id.clone(),
                    role: agent,
                    model,
                    effort,
                    call_id,
                    task,
                    task_id,
                    state: AgentState::Thinking,
                    running: Vec::new(),
                    started_at: now,
                    ended_at: None,
                };
                self.agents.insert(thread_id, agent);
            }
            ThreadEvent::AgentsWorking(on) => self.agents_working = on,
            ThreadEvent::Team(team) => {
                self.set_team(team, now);
                self.touch_team_runs();
            }
            // An agent not started in this chat still has its chat followed.
            ThreadEvent::Subagent { thread_id, event } => {
                self.agent_chats
                    .entry(thread_id)
                    .or_default()
                    .apply(*event, now);
            }
            ThreadEvent::Approval(request) => self.approvals.push(request),
            ThreadEvent::ApprovalClosed { id } => self.approvals.retain(|request| request.id != id),
            ThreadEvent::TitleChanged { title } => self.title = Some(title),
            ThreadEvent::Pins(pins) => self.pins = pins,
            ThreadEvent::ProposalSettled { proposal_id, state } => self.settle(&proposal_id, state),
            ThreadEvent::SkillChanged(change) => self.skill_changed(change),
            ThreadEvent::Undone { text } => {
                // The core forgot the message and all after it; so does the chat.
                if let Some(at) = self.rollback_at.take() {
                    self.truncate(at);
                }
                self.offered = Some(text);
            }
            // The rest left a note above, is the client's to show (a question waits for
            // its answer in the client), or is not shown.
            _ => {}
        }
    }

    /// The turn is over: what only a running turn has goes.
    fn end_turn(&mut self) {
        self.waiting = None;
        self.turn_started = None;
        self.spend = None;
        self.compacting = false;
    }

    /// Ends the steps still running as failed with `why`: a turn that ended while tools
    /// ran stopped them.
    pub fn stop_running_tools(&mut self, why: &str) {
        let running: Vec<usize> = (0..self.items.len())
            .filter(|&index| {
                matches!(&self.items[index], Item::Tool(tool) if tool.state == ToolState::Running)
            })
            .collect();
        for index in running {
            if let Item::Tool(tool) = &mut self.items[index] {
                tool.finish_stored(why);
                tool.state = ToolState::Failed;
            }
            self.touch(index);
        }
        let in_runs: Vec<String> = self
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Run(run) => Some(run),
                _ => None,
            })
            .flat_map(|run| &run.tools)
            .filter(|tool| tool.state == ToolState::Running)
            .map(|tool| tool.call_id.clone())
            .collect();
        for call_id in in_runs {
            self.finish_run_call(&call_id, |tool| {
                tool.finish_stored(why);
                tool.state = ToolState::Failed;
            });
        }
    }

    /// Cuts the items at `at`, as an /undo the core confirmed does.
    pub fn truncate(&mut self, at: usize) {
        if at >= self.items.len() {
            return;
        }
        self.items.truncate(at);
        self.meta.truncate(at);
        self.asked = Asked::default();
        self.showing.clear();
        self.proposing.clear();
        self.batch = None;
        let kept = self
            .activity_boundaries
            .iter()
            .take_while(|boundary| **boundary <= at)
            .count();
        self.activity_boundaries.truncate(kept);
        self.boundary_times.truncate(kept);
        self.boundary_spend.truncate(kept);
        self.reshape();
        self.mark(at);
    }

    /// A skill change that came live, a list read from the core, or one that was undone:
    /// it takes the place of the change with its ID. An undo is final: a stale list must
    /// not offer Undo again.
    pub fn skill_changed(&mut self, change: SkillChange) {
        if let Some(index) = self.tool_index(&change.call_id) {
            self.touch(index);
        }
        match self
            .skill_changes
            .iter_mut()
            .find(|known| known.id == change.id)
        {
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

    /// The changes the core lists for a thread opened from history; those that came live
    /// meanwhile stay. They are kept in the order they were made.
    pub fn skills_loaded(&mut self, changes: Vec<SkillChange>) {
        for change in changes {
            self.skill_changed(change);
        }
        self.skill_changes
            .sort_by_key(|change| (change.changed_at_ms, change.id));
    }

    /// A verdict on the goal, live or stored: it comes between turns and stands alone.
    fn goal_verdict(&mut self, evaluation: GoalEvaluation, at: i64) {
        self.boundary(at);
        self.push(Item::Goal(evaluation), at);
        self.boundary(at);
    }

    /// Draws a boundary after the last item; false when there is one there already.
    fn boundary(&mut self, at: i64) -> bool {
        let end = self.items.len();
        let new = end > 0 && self.activity_boundaries.last() != Some(&end);
        if new {
            self.activity_boundaries.push(end);
            self.boundary_times.push(at);
            self.boundary_spend.push(None);
            self.reshape();
        }
        new
    }

    /// The answer being written, when the last item is one and no turn began since.
    fn answer_index(&self) -> Option<usize> {
        let last = self.items.len().checked_sub(1)?;
        let open = self.activity_boundaries.last() != Some(&self.items.len());
        (open && matches!(self.items[last], Item::Assistant { .. })).then_some(last)
    }

    /// The answer being written: the last item, or a new one after a tool, a message or
    /// the start of a turn.
    fn assistant(&mut self, now: i64) -> usize {
        match self.answer_index() {
            Some(index) => index,
            None => self.push(
                Item::Assistant {
                    text: String::new(),
                    reasoning: String::new(),
                },
                now,
            ),
        }
    }

    fn stream(&mut self, index: usize, piece: &str, text: bool) {
        let before = self.items[index].blanks();
        if let Item::Assistant {
            text: said,
            reasoning,
        } = &mut self.items[index]
        {
            match text {
                true => said.push_str(piece),
                false => reasoning.push_str(piece),
            }
        }
        self.touch(index);
        if self.items[index].blanks() != before {
            self.reshape();
        }
    }

    fn summary(&mut self, piece: &str, now: i64) {
        match self.items.last_mut() {
            Some(Item::Summary(text)) => {
                text.push_str(piece);
                self.touch(self.items.len() - 1);
            }
            _ => {
                self.push(Item::Summary(piece.to_owned()), now);
            }
        }
    }

    /// A step that starts, live or read from history; a `show` call also keeps the block it
    /// asks for, which its result may put in the step's place.
    fn start_tool(&mut self, call: &ToolCall, state: ToolState, at: i64) {
        self.proposing.call(call);
        // A suggested next message is no step: the composer shows it, from
        // `NextSuggested`, and its result finds no row to land on.
        if call.name == SUGGEST_NEXT_TOOL {
            return;
        }
        self.asked.call(call);
        if let Some(block) = Block::asked(call) {
            self.showing.insert(call.id.clone(), block);
        }
        let tool = Tool::new(call, state);
        if !self.start_run(call, tool.clone(), at) {
            self.push(Item::Tool(tool), at);
        }
    }

    /// Puts the block of a finished `show` call in place of its step, when `result` says
    /// it was shown; a refused call stays a step with its error. Whether it did.
    fn show_block(&mut self, call_id: &str, result: &str) -> bool {
        let Some(block) = self
            .showing
            .remove(call_id)
            .filter(|block| block.shown_by(result))
        else {
            return false;
        };
        let Some(index) = self.tool_index(call_id) else {
            return false;
        };
        self.items[index] = Item::Block(block);
        self.touch(index);
        self.reshape();
        true
    }

    /// Puts the card of a finished `suggest_from_workshop` call in place of its step,
    /// when `result` holds one; whether it did.
    fn suggestion(&mut self, call_id: &str, result: &str) -> bool {
        let Some(index) = self.tool_index(call_id) else {
            return false;
        };
        if !matches!(&self.items[index], Item::Tool(tool) if tool.name == SUGGEST_TOOL) {
            return false;
        }
        let Some(card) = Suggestion::read(result) else {
            return false;
        };
        self.items[index] = Item::Suggestion(card);
        self.touch(index);
        self.reshape();
        true
    }

    /// Puts the card of a finished `propose_session` call in place of its step, when
    /// `result` holds one; whether it did.
    fn proposal(&mut self, call_id: &str, result: &str) -> bool {
        // Only the call's own result: text another tool returned is never a card.
        let Some(proposal) = self.proposing.card(call_id, result) else {
            return false;
        };
        let Some(index) = self.tool_index(call_id) else {
            return false;
        };
        if !matches!(&self.items[index], Item::Tool(tool) if tool.name == PROPOSE_SESSION_TOOL) {
            return false;
        }
        self.items[index] = Item::Proposal {
            proposal,
            state: ProposalState::Pending,
        };
        self.touch(index);
        self.reshape();
        true
    }

    /// Marks the card of the proposal `id` started or dismissed.
    fn settle(&mut self, id: &str, settled: ProposalState) {
        let index = self
            .items
            .iter()
            .rposition(|item| matches!(item, Item::Proposal { proposal, .. } if proposal.id == id));
        if let Some(index) = index
            && let Item::Proposal { state, .. } = &mut self.items[index]
        {
            *state = settled;
            self.touch(index);
        }
    }

    /// Whether `text`, a message from a thread, is the core's note of how a proposal of
    /// this chat was answered: it settles the card and is not drawn itself.
    fn settled_by(&mut self, text: &str) -> bool {
        let Some((id, state)) = read_outcome(text) else {
            return false;
        };
        let carded = self
            .items
            .iter()
            .any(|item| matches!(item, Item::Proposal { proposal, .. } if proposal.id == id));
        if carded {
            self.settle(&id, state);
        }
        carded
    }

    /// The pictures and sounds a tool or a message carries. Pictures of a tool other than
    /// `display_image` are aside: the model took them to see.
    fn add_pictures(&mut self, call_id: &str, cards: &[Card], at: i64) {
        let tool = self
            .tool_index(call_id)
            .and_then(|index| match &self.items[index] {
                Item::Tool(tool) => Some(tool.name.as_str()),
                _ => None,
            });
        let aside = tool.is_some_and(|name| name != "display_image");
        let call_id = (!call_id.is_empty()).then(|| call_id.to_owned());
        for card in cards {
            let item = match card.kind {
                CardKind::Audio => Item::Audio(card.clone()),
                CardKind::Image => Item::Image {
                    card: card.clone(),
                    call_id: call_id.clone(),
                    aside,
                },
            };
            self.push(item, at);
        }
    }
}

#[cfg(test)]
mod tests;
