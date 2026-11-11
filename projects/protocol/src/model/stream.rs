//! What a model streams.

use serde::{Deserialize, Serialize};

use crate::model::Usage;
/// One increment of a streamed model response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelEvent {
    /// Model thinking: shown to the user and never sent back as context.
    Reasoning(String),
    Text(String),
    /// A new tool call; its arguments follow as `ToolArguments` with the same index.
    ToolCall {
        index: u32,
        id: String,
        name: String,
    },
    /// A piece of the JSON arguments of the call with this index.
    ToolArguments {
        index: u32,
        fragment: String,
    },
    Usage(Usage),
    Finished(FinishReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    Stop,
    ToolCalls,
    /// Cut by the output token limit.
    Length,
    ContentFilter,
}
