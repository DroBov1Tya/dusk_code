//! Converts stored thread messages into model input: role and text only.

use crate::{
    providers::ChatMessage,
    state::{Message, MessageRole},
};

/// IDs, timestamps, cwd and attachments stay in the core and never reach the model.
pub(super) fn chat_messages(messages: &[Message]) -> Vec<ChatMessage> {
    messages.iter().filter_map(chat_message).collect()
}

/// Estimated size of what `chat_messages` sends.
pub(super) fn context_tokens(messages: &[Message]) -> u64 {
    messages
        .iter()
        .filter(|message| chat_message(message).is_some())
        .map(|message| message.token_count)
        .sum()
}

fn chat_message(message: &Message) -> Option<ChatMessage> {
    let content = message.content.clone();
    match message.role {
        MessageRole::System => Some(ChatMessage::System(content)),
        MessageRole::User => Some(ChatMessage::User(content)),
        MessageRole::Assistant | MessageRole::Agent => Some(ChatMessage::Assistant {
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
            tool_calls: Vec::new(),
            tool_call_id: (role == MessageRole::Tool).then(|| "call".into()),
            cwd: "/tmp".into(),
            attachments: Vec::new(),
            token_count: 5,
            created_at: 0,
        }
    }

    #[test]
    fn keeps_only_role_text_and_tool_links() {
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
        assert_eq!(context_tokens(&messages), 25);
    }
}
