//! A chat read from its stored messages.

use super::{ChatLog, Item};
use crate::thread::{JobResult, Message, MessageRole, ToolState};

impl<L> ChatLog<L> {
    /// The chat of a thread's stored messages, with the boundaries a live chat draws: a
    /// human message ends what came before it and opens a turn after it, as `TurnStarted`
    /// does once the core took the message in; an answer without calls ends the turn. The
    /// stored answer to the agent's `ask_user` question goes on with the turn that asked.
    pub fn from_history(history: &[Message]) -> Self {
        let mut chat = Self::default();
        let mut question = None;
        for message in history {
            let mut opens = false;
            if message.role == MessageRole::User && message.from_thread_id.is_none() {
                // The core admits only a matching answer into an active question. It
                // persists as a User message, so recognize the stored call and its exact
                // encoded prompt together.
                let is_answer = question.is_some_and(|prompt| {
                    message
                        .content
                        .starts_with(&format!("QUESTION: {prompt}\nANSWER: "))
                });
                if !is_answer {
                    // A turn the history left without an answer ends here.
                    chat.boundary(message.created_at);
                    opens = true;
                }
                question = None;
            }
            chat.add_message(message, message.created_at);
            if opens {
                chat.boundary(message.created_at);
            }
            // Persisted model steps carry calls; the final response is a separate
            // assistant message. A new human prompt also ends legacy partial turns.
            if message.role == MessageRole::Assistant {
                if message.tool_calls.is_empty() {
                    chat.boundary(message.created_at);
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
        chat
    }

    /// One stored message, read in from history or as it came live.
    pub(super) fn add_message(&mut self, message: &Message, at: i64) {
        let named = |chat: &mut Self, index: usize| {
            chat.meta[index].message_id = Some(message.message_id.clone());
        };
        match message.role {
            MessageRole::System => {}
            MessageRole::User
                if message.from_thread_id.is_some() && self.settled_by(&message.content) => {}
            MessageRole::User if message.from_thread_id.is_some() => {
                let from = message.from_thread_id.as_deref().unwrap_or_default();
                let item = Item::Agent {
                    from: self.agent_name(from),
                    thread_id: message.from_thread_id.clone(),
                    text: message.content.clone(),
                };
                self.push(item, at);
            }
            MessageRole::User => {
                let index = self.add_user(message.content.clone(), Some(message.sequence), at);
                named(self, index);
            }
            MessageRole::Job => {
                self.push(Item::Job(JobResult::read(&message.content)), at);
            }
            MessageRole::Goal => {
                if let Ok(evaluation) = serde_json::from_str(&message.content) {
                    self.goal_verdict(evaluation, at);
                }
            }
            MessageRole::Assistant => {
                // Each stored answer is a step of its own.
                self.batch = None;
                if !message.content.is_empty() || message.reasoning.is_some() {
                    let item = Item::Assistant {
                        text: message.content.clone(),
                        reasoning: message.reasoning.clone().unwrap_or_default(),
                    };
                    let index = self.push(item, at);
                    named(self, index);
                }
                for call in &message.tool_calls {
                    self.start_tool(call, ToolState::Done, at);
                }
            }
            MessageRole::Tool => {
                let call_id = message.tool_call_id.as_deref().unwrap_or_default();
                if !self.suggestion(call_id, &message.content)
                    && !self.proposal(call_id, &message.content)
                    && !self.finish_run_call(call_id, |tool| tool.finish_stored(&message.content))
                    && !self.show_block(call_id, &message.content)
                    && let Some(index) = self.tool_index(call_id)
                {
                    if let Item::Tool(tool) = &mut self.items[index] {
                        tool.finish_stored(&message.content);
                    }
                    self.touch(index);
                }
            }
        }
        let call_id = message.tool_call_id.as_deref().unwrap_or_default();
        self.add_pictures(call_id, &message.artifacts, at);
    }
}
