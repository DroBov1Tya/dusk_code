/// Input delivered to the core. Message content exists only in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    UserMessage {
        request_uid: String,
        thread_id: String,
        content: String,
    },
}
