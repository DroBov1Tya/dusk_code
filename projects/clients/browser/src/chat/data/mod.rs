//! What one chat shows: the protocol's chat, folded from its history and then from its
//! live events, with what only this page keeps beside it (messages it sent that the core
//! has not taken in yet, the question being answered). Plain data with no browser in it,
//! so the drawing code only reads it and the rules can be tested.

use dusk_protocol::{
    config::ApprovalPolicy,
    thread::{Applied, ChatLog, Message, ThreadEvent},
    tools::Question,
};
use leptos::prelude::{ArcTrigger, Notify, Track};
use std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
};

pub use dusk_protocol::thread::transcript::Item;

pub struct ChatData {
    pub log: ChatLog,
    /// Counts the turns that ended and the plan's and team's changes, so the open
    /// Overview knows to ask again.
    pub moved: u64,
    pub access: Option<ApprovalPolicy>,
    pub question: Option<Question>,
    /// Questions asked while one was on the page, and the answers given so far: they go
    /// to the agent together, after the last one.
    pub questions_waiting: VecDeque<Question>,
    pub answers: Vec<String>,
    /// Messages this page sent that the core has not taken into the chat yet.
    pub pending: Vec<String>,
    /// The stream broke or the thread ended; sending opens it again.
    pub ended: bool,
    /// Fires when only one item's content changed, as a streamed piece of the answer
    /// does; the chat's own signal then stays quiet.
    pub streamed: ArcTrigger,
    /// Fires when only a subagent's chat changed.
    pub subagents: ArcTrigger,
}

impl Default for ChatData {
    fn default() -> Self {
        Self {
            log: ChatLog::default(),
            moved: 0,
            access: None,
            question: None,
            questions_waiting: VecDeque::new(),
            answers: Vec::new(),
            pending: Vec::new(),
            ended: false,
            streamed: ArcTrigger::new(),
            subagents: ArcTrigger::new(),
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
    /// Keeps the answer to the question shown and shows the next one. The message for
    /// the agent once every question asked has its answer.
    pub fn record_answer(&mut self, reply: String) -> Option<String> {
        self.answers.push(reply);
        self.question = self.questions_waiting.pop_front();
        if self.question.is_none() {
            self.live.answered();
        }
        self.question
            .is_none()
            .then(|| std::mem::take(&mut self.answers).join("\n\n"))
    }

    pub fn from_history(history: &[Message]) -> Self {
        Self {
            log: ChatLog::from_history(history),
            ..Self::default()
        }
    }

    /// The page just sent `text`; it shows as queued until the core takes it in.
    pub fn sent(&mut self, text: String) {
        self.pending.push(text);
    }

    pub fn apply(&mut self, event: ThreadEvent) -> Applied {
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
            // A second question waits behind the one on the page.
            ThreadEvent::Question(question) => {
                match self.question {
                    Some(_) => self.questions_waiting.push_back(question),
                    None => self.question = Some(question),
                }
                return Applied::Any;
            }
            _ => {}
        }
        self.log.apply(event, now_ms())
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
