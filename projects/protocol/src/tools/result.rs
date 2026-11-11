//! A tool's result.

use serde::{Deserialize, Serialize};

use crate::tools::Card;
/// What the model gets back from a call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    /// Set only by the foreground shell runner, never inferred from output text.
    pub exit_code: Option<i32>,
    pub artifacts: Vec<Card>,
}

impl ToolResult {
    pub fn text(content: String) -> Self {
        Self {
            content,
            is_error: false,
            exit_code: None,
            artifacts: Vec::new(),
        }
    }
}

/// Window inventory or one kind of access to a retained window selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowAccess {
    List,
    Capture(String),
    Activate(String),
}
