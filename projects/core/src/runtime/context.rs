//! A thread as the model sees it: its messages as role and text, and which of them are
//! still waiting for an answer.

use anyhow::Result;

use crate::{
    providers::ChatMessage,
    state::{Message, MessageRole, State},
};

pub(super) struct Conversation {
    pub(super) messages: Vec<Message>,
    /// Sequence of the newest message a turn has taken; user messages after it are queued.
    pub(super) answered_through: u64,
}

impl Conversation {
    pub(super) async fn load(state: &State, thread_id: &str) -> Result<Self> {
        Ok(Self {
            answered_through: state.answered_through(thread_id).await?,
            messages: state.list_messages(thread_id).await?,
        })
    }

    pub(super) fn has_queued(&self) -> bool {
        self.messages.iter().any(|message| {
            message.role == MessageRole::User && message.sequence > self.answered_through
        })
    }

    /// IDs, timestamps and other metadata stay in the core and never reach the model.
    pub(super) fn chat_messages(&self) -> Vec<ChatMessage> {
        self.messages.iter().filter_map(chat_message).collect()
    }

    /// Estimated size of what `chat_messages` sends.
    pub(super) fn tokens(&self) -> u64 {
        self.messages
            .iter()
            .filter(|message| chat_message(message).is_some())
            .map(|message| message.token_count)
            .sum()
    }
}

fn chat_message(message: &Message) -> Option<ChatMessage> {
    let content = message.content.clone();
    match message.role {
        MessageRole::System => Some(ChatMessage::System(content)),
        MessageRole::User => Some(ChatMessage::User(content)),
        MessageRole::Assistant => Some(ChatMessage::Assistant {
            text: content,
            tool_calls: message.tool_calls.clone(),
        }),
        MessageRole::Tool => Some(ChatMessage::Tool {
            call_id: message.tool_call_id.clone()?,
            content,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(sequence: u64, role: MessageRole, content: &str) -> Message {
        Message {
            message_id: format!("id-{sequence}"),
            thread_id: "thread".into(),
            sequence,
            role,
            content: content.into(),
            reasoning: None,
            tool_calls: Vec::new(),
            tool_call_id: (role == MessageRole::Tool).then(|| "call".into()),
            token_count: 5,
            created_at: 0,
        }
    }

    #[test]
    fn sends_role_text_and_tool_links_only() {
        let conversation = Conversation {
            messages: vec![
                message(1, MessageRole::System, "prompt"),
                message(2, MessageRole::User, "question"),
                message(3, MessageRole::Assistant, "answer"),
                message(4, MessageRole::Tool, "tool output"),
                message(5, MessageRole::User, "follow-up"),
            ],
            answered_through: 2,
        };
        assert_eq!(
            conversation.chat_messages(),
            [
                ChatMessage::System("prompt".into()),
                ChatMessage::User("question".into()),
                ChatMessage::Assistant {
                    text: "answer".into(),
                    tool_calls: Vec::new()
                },
                ChatMessage::Tool {
                    call_id: "call".into(),
                    content: "tool output".into()
                },
                ChatMessage::User("follow-up".into()),
            ]
        );
        assert_eq!(conversation.tokens(), 25);
        assert!(conversation.has_queued(), "the follow-up waits");
    }
}
