//! The tools a thread offers, by name: built-in ones and those of MCP servers.

use std::{collections::BTreeMap, path::Path, time::Instant};

use serde_json::Value;
use tracing::info;

use super::{
    scope::ToolScope,
    tool::{Progress, Tool, ToolContext, ToolKind},
};
use crate::{
    config::Config,
    providers::{ToolCall, ToolSpec},
};

/// The model reads every result; a runaway one must not flood its context.
const MAX_RESULT_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Box<dyn Tool>>,
}

/// What the model gets back from a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

impl ToolRegistry {
    /// The built-in tools; `skills` is the skill directory `load_skill` reads.
    pub fn builtin(skills: &Path) -> Self {
        let mut registry = Self::default();
        for tool in super::builtin::all(skills) {
            registry.add(tool);
        }
        registry
    }

    /// A later tool with the same name replaces the earlier one.
    pub fn add(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_owned(), tool);
    }

    /// Drops every tool not named in `names`.
    pub fn keep(&mut self, names: &[String]) {
        self.tools
            .retain(|name, _| names.iter().any(|kept| kept == name));
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tools.keys().map(String::as_str)
    }

    /// Every tool with its kind, by kind and then by name.
    pub fn kinds(&self) -> Vec<(String, ToolKind)> {
        let mut kinds: Vec<_> = self
            .tools
            .iter()
            .map(|(name, tool)| (name.clone(), tool.kind()))
            .collect();
        kinds.sort_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)));
        kinds
    }

    /// The tools the config has on, as the model sees them.
    pub fn specs(&self, scope: &ToolScope, config: &Config) -> Vec<ToolSpec> {
        let context = ToolContext {
            scope,
            config,
            progress: None,
        };
        self.tools
            .values()
            .filter(|tool| config.tool_enabled(tool.name()) && tool.available(&context))
            .map(|tool| ToolSpec {
                name: tool.name().to_owned(),
                description: tool.description(&context),
                parameters: tool.parameters(),
            })
            .collect()
    }

    /// Never fails: an unknown, switched-off or failed call becomes an error result for
    /// the model. A model may call a tool it was not offered, so the switch is checked here.
    pub async fn call(
        &self,
        call: &ToolCall,
        scope: &ToolScope,
        config: &Config,
        progress: Progress<'_>,
    ) -> ToolResult {
        let Some(tool) = self.tools.get(call.name.as_str()) else {
            return error_result(format!("unknown tool {}", call.name));
        };
        if !config.tool_enabled(tool.name()) {
            return error_result(format!("tool {} is switched off", call.name));
        }
        let arguments = match &call.arguments {
            Value::Object(_) => call.arguments.clone(),
            Value::String(raw) => {
                let error = serde_json::from_str::<Value>(raw).err();
                let reason = error.map_or("not an object".into(), |error| error.to_string());
                return error_result(format!("arguments are not valid JSON ({reason})"));
            }
            _ => return error_result("arguments must be a JSON object".into()),
        };
        let context = ToolContext {
            scope,
            config,
            progress: Some(progress),
        };
        let started = Instant::now();
        let result = tool.call(arguments, context).await;
        let took = started.elapsed().as_millis();
        match result {
            Ok(content) => {
                info!("tool {} done in {took} ms", call.name);
                ToolResult {
                    content: bounded(content),
                    is_error: false,
                }
            }
            Err(error) => {
                // Name and time only: arguments, output and errors may quote files.
                info!("tool {} failed in {took} ms", call.name);
                error_result(format!("{error:#}"))
            }
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
