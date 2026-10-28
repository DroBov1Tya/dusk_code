//! Converts stored thread messages into model input: role and text only.

use crate::{
    providers::{ChatMessage, ChatRole},
    state::{Message, MessageRole},
};

/// IDs, timestamps, cwd and attachments stay in the core and never reach the model.
pub(super) fn chat_messages(messages: &[Message]) -> Vec<ChatMessage> {
    messages
        .iter()
        .filter_map(|message| {
            Some(ChatMessage {
                role: chat_role(message.role)?,
                content: message.content.clone(),
            })
        })
        .collect()
}

/// Estimated size of what `chat_messages` sends.
pub(super) fn context_tokens(messages: &[Message]) -> u64 {
    messages
        .iter()
        .filter(|message| chat_role(message.role).is_some())
        .map(|message| message.token_count)
        .sum()
}

fn chat_role(role: MessageRole) -> Option<ChatRole> {
    match role {
        MessageRole::System => Some(ChatRole::System),
        MessageRole::User => Some(ChatRole::User),
        MessageRole::Assistant | MessageRole::Agent => Some(ChatRole::Assistant),
        // No tools exist yet.
        MessageRole::Tool => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(role: MessageRole, content: &str) -> Message {
        Message {
            message_id: format!("id-{content}"),
            thread_id: "thread".into(),
            job_id: None,
            request_uid: Some("uid".into()),
            sequence: 1,
            role,
            agent_id: None,
            content: content.into(),
            reasoning: None,
            cwd: "/tmp".into(),
            attachments: Vec::new(),
            token_count: 5,
            created_at: 0,
        }
    }

    #[test]
    fn keeps_only_role_and_text_of_conversation_messages() {
        let messages = [
            message(MessageRole::System, "prompt"),
            message(MessageRole::User, "question"),
            message(MessageRole::Assistant, "answer"),
            message(MessageRole::Tool, "tool output"),
            message(MessageRole::User, "follow-up"),
        ];
        assert_eq!(
            chat_messages(&messages),
            [
                ChatMessage {
                    role: ChatRole::System,
                    content: "prompt".into()
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: "question".into()
                },
                ChatMessage {
                    role: ChatRole::Assistant,
                    content: "answer".into()
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: "follow-up".into()
                },
            ]
        );
        assert_eq!(context_tokens(&messages), 20);
    }
}
