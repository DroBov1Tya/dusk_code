//! A tool call a model makes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-assigned; a tool result refers back to it.
    pub id: String,
    pub name: String,
    pub arguments: Value,
}
