use crate::state::MessageAttachment;

/// Input delivered to the core. Accepted messages are persisted with their receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    UserMessage {
        request_uid: String,
        thread_id: String,
        content: String,
        cwd: String,
        attachments: Vec<MessageAttachment>,
    },
}
