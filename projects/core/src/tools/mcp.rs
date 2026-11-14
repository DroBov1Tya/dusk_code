//! A tool an MCP server offers, under the name `server__tool`. Its description and
//! schema are the server's; what it returns is untrusted text like any tool result.

use std::sync::Arc;

use serde_json::Value;

use super::{Tool, ToolContext, ToolKind, registry::ToolResult};
use crate::{BoxFuture, mcp::McpServer, state::artifacts::ArtifactStore};

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
    /// Where the pictures it returns are kept for the model.
    pictures: ArtifactStore,
}

impl McpTool {
    /// Every tool of the server whose name fits what models accept.
    pub fn all(server: &Arc<McpServer>, pictures: &ArtifactStore) -> Vec<Self> {
        server
            .tools
            .iter()
            .filter_map(|tool| {
                Some(Self {
                    server: Arc::clone(server),
                    name: Self::model_name(&server.name, &tool.name)?,
                    remote: tool.name.clone(),
                    description: tool.description.clone(),
                    schema: tool.input_schema.clone(),
                    pictures: pictures.clone(),
                })
            })
            .collect()
    }

    /// The name the model calls a server's tool by; `None` when it would be too long.
    pub fn model_name(server: &str, remote: &str) -> Option<String> {
        let name = format!("{server}__{}", tool_name(remote));
        (name.len() <= MAX_NAME).then_some(name)
    }
}

impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Mcp
    }

    fn description(&self, _context: &ToolContext) -> String {
        format!(
            "From the MCP server {}. {}",
            self.server.name, self.description
        )
    }

    fn parameters(&self) -> Value {
        self.schema.clone()
    }

    fn call<'a>(&'a self, arguments: Value, _context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            Ok(self
                .server
                .client
                .call_tool(&self.remote, arguments)
                .await?
                .text)
        })
    }

    /// Pictures in the result become cards the model sees, such as a device's screen.
    fn call_result<'a>(
        &'a self,
        arguments: Value,
        _context: ToolContext,
    ) -> BoxFuture<'a, ToolResult> {
        Box::pin(async move {
            let output = self
                .server
                .client
                .call_tool(&self.remote, arguments)
                .await?;
            let mut result = ToolResult::text(output.text);
            for bytes in output.images {
                let store = self.pictures.clone();
                let title = self.server.name.clone();
                let card = tokio::task::spawn_blocking(move || {
                    super::harness::picture_card(&store, bytes, &title)
                })
                .await?;
                match card {
                    Ok(card) => result.artifacts.push(card),
                    Err(error) => result
                        .content
                        .push_str(&format!("\n[a picture that could not be read: {error}]")),
                }
            }
            Ok(result)
        })
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
