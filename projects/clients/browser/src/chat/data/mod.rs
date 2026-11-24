//! What one chat shows: the protocol's chat, folded from its history and then from its
//! live events, with what only this page keeps beside it (messages it sent that the core
//! has not taken in yet, the question being answered). Plain data with no browser in it,
//! so the drawing code only reads it and the rules can be tested.

use dusk_protocol::{
    config::ApprovalPolicy,
    thread::{Applied, ChatLog, Goal, Message, Suggestion, ThreadEvent},
    tools::QuestionQueue,
};
use leptos::prelude::{ArcTrigger, Notify, Track};
use std::ops::{Deref, DerefMut};

pub use dusk_protocol::thread::transcript::Item;

pub struct ChatData {
    pub log: ChatLog,
    /// Counts the turns that ended and the plan's and team's changes, so the open
    /// Overview knows to ask again.
    pub moved: u64,
    pub access: Option<ApprovalPolicy>,
    /// The question on the page, those asked while it was, and the answers given so far.
    pub questions: QuestionQueue,
    /// Messages this page sent that the core has not taken into the chat yet.
    pub pending: Vec<String>,
    /// The stream broke or the thread ended; sending opens it again.
    pub ended: bool,
    /// The user's likely next message, as the model proposed it, for the composer.
    pub suggestion: Suggestion,
    /// Fires when only one item's content changed, as a streamed piece of the answer
    /// does; the chat's own signal then stays quiet.
    pub streamed: ArcTrigger,
    /// Fires when only a subagent's chat changed.
    pub subagents: ArcTrigger,
    /// The chat's standing goal, as the core last told it.
    pub goal: Option<Goal>,
}

impl Default for ChatData {
    fn default() -> Self {
        Self {
            log: ChatLog::default(),
            moved: 0,
            access: None,
            questions: QuestionQueue::default(),
            pending: Vec::new(),
            ended: false,
            suggestion: Suggestion::default(),
            streamed: ArcTrigger::new(),
            subagents: ArcTrigger::new(),
            goal: None,
        }
    }
}

impl Deref for ChatData {
    type Target = ChatLog;

    fn deref(&self) -> &ChatLog {
        &self.log
    }
}

impl DerefMut for ChatData {
    fn deref_mut(&mut self) -> &mut ChatLog {
        &mut self.log
    }
}

impl ChatData {
    /// Answers the questions through `act`: the message for the agent once every question
    /// asked has its answer, and then the turn no longer waits on the user.
    pub fn answer_questions(
        &mut self,
        act: impl FnOnce(&mut QuestionQueue) -> Option<String>,
    ) -> Option<String> {
        let sent = act(&mut self.questions);
        if sent.is_some() {
            self.live.answered();
        }
        sent
    }

    pub fn from_history(history: &[Message]) -> Self {
        Self {
            log: ChatLog::from_history(history),
            ..Self::default()
        }
    }

    /// The chat is about to be opened again after the stream broke. The goal may have
    /// ended meanwhile, and the core tells it on opening only when there is one, so the
    /// old one goes until the opening tells it again.
    pub fn reopening(&mut self) {
        self.goal = None;
    }

    /// The history read again after reopening; the tasks, the access and the goal the
    /// opening told, which the history does not hold, stay.
    pub fn reloaded(&mut self, history: &[Message]) {
        let (tasks, access, goal) = (self.tasks.clone(), self.access, self.goal.take());
        *self = Self::from_history(history);
        (self.tasks, self.access, self.goal) = (tasks, access, goal);
    }

    /// The page just sent `text`; it shows as queued until the core takes it in.
    pub fn sent(&mut self, text: String) {
        self.pending.push(text);
    }

    pub fn apply(&mut self, event: ThreadEvent) -> Applied {
        self.suggestion.hear(&event);
        if matches!(
            event,
            ThreadEvent::TurnFinished(_) | ThreadEvent::TaskChanged(_) | ThreadEvent::Team(_)
        ) {
            self.moved += 1;
        }
        match event {
            ThreadEvent::UserMessage(ref message) => {
                if let Some(at) = self.pending.iter().position(|p| *p == message.content) {
                    self.pending.remove(at);
                }
            }
            // The core answers a `/goal` with the goal or a rejection, never as a message.
            ThreadEvent::Goal { ref goal, .. } => {
                self.goal = goal.clone();
                self.answered_goal_command();
            }
            ThreadEvent::Rejected { .. } => self.answered_goal_command(),
            // The verdict carries what the goal has cost so far and the newest reason.
            ThreadEvent::GoalEvaluated(ref evaluation) => {
                if let Some(goal) = &mut self.goal {
                    goal.turns = evaluation.turn;
                    goal.tokens = evaluation.tokens;
                    goal.reason = Some(evaluation.reason.clone());
                }
            }
            // A second question waits behind the one on the page.
            ThreadEvent::Question(question) => {
                self.questions.ask(question);
                return Applied::Any;
            }
            _ => {}
        }
        self.log.apply(event, now_ms())
    }

    /// The oldest `/goal` this page sent is answered: it no longer waits as queued.
    fn answered_goal_command(&mut self) {
        let command = |text: &String| text == "/goal" || text.starts_with("/goal ");
        if let Some(at) = self.pending.iter().position(command) {
            self.pending.remove(at);
        }
    }

    /// Tells what `applied` changed to the views that read it: only the item, only the
    /// subagent windows, or the whole chat.
    pub fn told(&self, applied: &Applied) -> bool {
        match applied {
            Applied::Item(_) => self.streamed.notify(),
            Applied::Subagent(_) => self.subagents.notify(),
            Applied::Any => return false,
        }
        true
    }

    /// Tracks the changes `told` gives out alone: a view of items follows streamed text,
    /// one of a subagent's chat follows the subagents too.
    pub fn track_streams(&self, subagents: bool) {
        self.streamed.track();
        if subagents {
            self.subagents.track();
        }
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
