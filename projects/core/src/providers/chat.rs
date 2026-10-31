//! What the model receives: the conversation and the tools it may call.

use serde_json::Value;

use super::model_output::ToolCall;
use crate::config::Effort;

/// A picture as the model gets it.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub mime_type: &'static str,
    pub base64: String,
}

/// One message as the model sees it: role and text, without core metadata.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ChatMessage {
    System(String),
    User(String),
    /// A user message with pictures after its text.
    UserWithPictures {
        text: String,
        pictures: Vec<Picture>,
    },
    Assistant {
        text: String,
        tool_calls: Vec<ToolCall>,
    },
    Tool {
        call_id: String,
        content: String,
    },
}

/// A tool the model may call, described the way models are trained to read it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments object.
    pub parameters: Value,
}

#[derive(Debug, Clone, Copy)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [ChatMessage],
    pub tools: &'a [ToolSpec],
    /// The provider stops the model after this many tokens.
    pub max_output_tokens: u64,
    pub effort: Option<Effort>,
}
