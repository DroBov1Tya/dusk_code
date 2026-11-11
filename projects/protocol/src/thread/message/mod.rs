//! The messages of a thread.

use serde::{Deserialize, Serialize};

use crate::model::ToolCall;
use crate::tools::Card;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub message_id: String,
    pub thread_id: String,
    /// Order within the thread, from 1.
    pub sequence: u64,
    pub role: MessageRole,
    pub content: String,
    /// Model thinking; only assistant messages carry it, and only when it was recorded.
    pub reasoning: Option<String>,
    /// Only assistant messages call tools.
    pub tool_calls: Vec<ToolCall>,
    /// Set exactly on tool messages: the call they answer.
    pub tool_call_id: Option<String>,
    /// Image cards for clients. Never included in model context or URL detection.
    pub artifacts: Vec<Card>,
    /// For a user message another agent sent: the thread it came from.
    pub from_thread_id: Option<String>,
    /// A process completion, distinct from user input and system instructions.
    pub from_job_id: Option<String>,
    /// Estimated sizes in model tokens; see `estimate_tokens`.
    pub content_tokens: u64,
    pub tool_calls_tokens: u64,
    pub reasoning_tokens: u64,
    /// Unix timestamp in milliseconds.
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
    Job,
}

impl Message {
    /// Estimated size of what the model receives of this message.
    pub fn context_tokens(&self) -> u64 {
        self.content_tokens + self.tool_calls_tokens
    }
}

impl MessageRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System | Self::Job => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

mod compaction;
mod pasted;
mod source;
mod undo;
mod unsent;

pub use compaction::Compaction;
pub use pasted::{Part, has_pasted, plain, source_tag, split, wrap};
pub use source::Source;
pub use undo::UndoPoint;
pub use unsent::LocalUnsent;
