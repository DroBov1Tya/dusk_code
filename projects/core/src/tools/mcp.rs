//! A tool an MCP server offers, under the name `server__tool`. Its description and
//! schema are the server's; what it returns is untrusted text like any tool result.

use std::sync::Arc;

use serde_json::Value;

use super::{Tool, ToolContext, ToolKind};
use crate::{BoxFuture, mcp::McpServer};

/// Models accept tool names this long at most.
const MAX_NAME: usize = 64;

pub struct McpTool {
    server: Arc<McpServer>,
    /// The name the model calls.
    name: String,
    /// The name the server knows.
    remote: String,
    description: String,
    schema: Value,
}

impl McpTool {
    /// Every tool of the server whose name fits what models accept.
    pub fn all(server: &Arc<McpServer>) -> Vec<Self> {
        server
            .tools
            .iter()
            .filter_map(|tool| {
                let name = format!("{}__{}", server.name, tool_name(&tool.name));
                (name.len() <= MAX_NAME).then(|| Self {
                    server: Arc::clone(server),
                    name,
                    remote: tool.name.clone(),
                    description: tool.description.clone(),
                    schema: tool.input_schema.clone(),
                })
            })
            .collect()
    }
}

impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Mcp
    }

    fn description(&self, _context: &ToolContext<'_>) -> String {
        format!(
            "From the MCP server {}. {}",
            self.server.name, self.description
        )
    }

    fn parameters(&self) -> Value {
        self.schema.clone()
    }

    fn call<'a>(&'a self, arguments: Value, _context: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(self.server.client.call_tool(&self.remote, arguments))
    }
}

/// Letters, digits, `_` and `-` stay; anything else becomes `_`.
fn tool_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
