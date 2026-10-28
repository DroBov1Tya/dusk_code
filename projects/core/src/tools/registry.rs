//! The tools a session offers, by name. Built-in tools now; MCP servers and switching
//! tools on and off later add to the same registry.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{scope::ToolScope, tool::Tool};
use crate::{
    config::Config,
    providers::{ToolCall, ToolSpec},
};

/// The model reads every result; a runaway one must not flood its context.
const MAX_RESULT_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<&'static str, Box<dyn Tool>>,
}

/// What the model gets back from a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

impl ToolRegistry {
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        for tool in super::builtin::all() {
            registry.add(tool);
        }
        registry
    }

    /// A later tool with the same name replaces the earlier one.
    pub fn add(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name(), tool);
    }

    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.tools.keys().copied()
    }

    /// The tools the config has on, as the model sees them.
    pub fn specs(&self, scope: &ToolScope, config: &Config) -> Vec<ToolSpec> {
        self.tools
            .values()
            .filter(|tool| config.tool_enabled(tool.name()))
            .map(|tool| ToolSpec {
                name: tool.name().to_owned(),
                description: tool.description(scope),
                parameters: tool.parameters(),
            })
            .collect()
    }

    /// Never fails: an unknown, switched-off or failed call becomes an error result for
    /// the model. A model may call a tool it was not offered, so the switch is checked here.
    pub async fn call(&self, call: &ToolCall, scope: &ToolScope, config: &Config) -> ToolResult {
        let Some(tool) = self.tools.get(call.name.as_str()) else {
            return error_result(format!("unknown tool {}", call.name));
        };
        if !config.tool_enabled(tool.name()) {
            return error_result(format!("tool {} is switched off", call.name));
        }
        let arguments = match &call.arguments {
            Value::Object(_) => call.arguments.clone(),
            _ => return error_result("arguments must be a JSON object".into()),
        };
        match tool.call(arguments, scope).await {
            Ok(content) => ToolResult {
                content: bounded(content),
                is_error: false,
            },
            Err(error) => error_result(format!("{error:#}")),
        }
    }
}

fn error_result(message: String) -> ToolResult {
    ToolResult {
        content: bounded(format!("error: {message}")),
        is_error: true,
    }
}

fn bounded(mut content: String) -> String {
    if content.trim().is_empty() {
        return "(no output)".into();
    }
    if content.len() > MAX_RESULT_BYTES {
        let mut end = MAX_RESULT_BYTES;
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        content.truncate(end);
        content.push_str("\n[output truncated]");
    }
    content
}
