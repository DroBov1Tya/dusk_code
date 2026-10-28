//! A thread as the model sees it: its messages as role and text, and which of them are
//! still waiting for an answer.

use anyhow::Result;

use crate::{
    providers::{ChatMessage, ToolSpec},
    state::{LastPrompt, Message, MessageRole, State, estimate_tokens},
};

pub(super) struct Conversation {
    pub(super) messages: Vec<Message>,
    /// Sequence of the newest message a turn has taken; user messages after it are queued.
    pub(super) answered_through: u64,
    pub(super) last_prompt: Option<LastPrompt>,
}

impl Conversation {
    pub(super) async fn load(state: &State, thread_id: &str) -> Result<Self> {
        Ok(Self {
            answered_through: state.answered_through(thread_id).await?,
            messages: state.list_messages(thread_id).await?,
            last_prompt: state.last_prompt(thread_id).await?,
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

    /// Sequence of the newest message `chat_messages` sends.
    pub(super) fn newest_sequence(&self) -> Option<u64> {
        self.messages.last().map(|message| message.sequence)
    }

    /// Size of a request with these messages and tools. The provider's count of the last
    /// request is exact, so only what came after it is estimated; estimates alone tend
    /// to miss the chat template markup.
    pub(super) fn tokens(&self, specs: &[ToolSpec]) -> u64 {
        // Compaction will have to drop this count, since it changes the earlier messages.
        let Some(last) = self.last_prompt else {
            return self.estimated_after(0) + spec_tokens(specs);
        };
        last.prompt_tokens + self.estimated_after(last.through_sequence)
    }

    fn estimated_after(&self, sequence: u64) -> u64 {
        self.messages
            .iter()
            .filter(|message| message.sequence > sequence && chat_message(message).is_some())
            .map(Message::context_tokens)
            .sum()
    }
}

/// Estimated size of the tool definitions a request carries.
pub(super) fn spec_tokens(specs: &[ToolSpec]) -> u64 {
    specs
        .iter()
        .map(|spec| {
            estimate_tokens(&spec.name)
                + estimate_tokens(&spec.description)
                + estimate_tokens(&spec.parameters.to_string())
        })
        .sum()
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
            content_tokens: 5,
            tool_calls_tokens: 0,
            reasoning_tokens: 0,
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
            last_prompt: None,
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
        assert_eq!(conversation.tokens(&[]), 25);
        assert!(conversation.has_queued(), "the follow-up waits");
    }

    #[test]
    fn the_last_prompt_count_replaces_estimates_up_to_its_message() {
        let conversation = Conversation {
            messages: (1..=4)
                .map(|sequence| message(sequence, MessageRole::User, "text"))
                .collect(),
            answered_through: 0,
            last_prompt: Some(LastPrompt {
                prompt_tokens: 100,
                through_sequence: 3,
            }),
        };
        let spec = ToolSpec {
            name: "tool".into(),
            description: "Does things.".into(),
            parameters: serde_json::json!({}),
        };

        // The tool definition is inside the provider's count already.
        assert_eq!(conversation.tokens(&[spec]), 100 + 5);
    }
}
