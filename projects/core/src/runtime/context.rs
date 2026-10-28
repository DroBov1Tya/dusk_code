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
            let role = match message.role {
                MessageRole::System => ChatRole::System,
                MessageRole::User => ChatRole::User,
                MessageRole::Assistant | MessageRole::Agent => ChatRole::Assistant,
                // No tools exist yet.
                MessageRole::Tool => return None,
            };
            Some(ChatMessage {
                role,
                content: message.content.clone(),
            })
        })
        .collect()
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
    }
}
