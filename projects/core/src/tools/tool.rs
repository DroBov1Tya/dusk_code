//! The tool interface: what the model reads about a tool, and the call that turns JSON
//! arguments into text for the model.

use std::{future::Future, pin::Pin};

use anyhow::Result;
use serde_json::Value;

use super::scope::ToolScope;
use crate::config::Config;

/// Boxed so tools of different kinds, built-in or MCP, fit in one registry.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;

/// What a tool sees: where it may act and the settings of the current step.
#[derive(Clone, Copy)]
pub struct ToolContext<'a> {
    pub scope: &'a ToolScope,
    pub config: &'a Config,
}

pub trait Tool: Send + Sync {
    /// Unique across the registry; what the model calls.
    fn name(&self) -> &'static str;

    /// Written for the model: what the tool does and when to use it. The context lets it
    /// name the real directories.
    fn description(&self, context: &ToolContext<'_>) -> String;

    /// JSON Schema of the arguments object.
    fn parameters(&self) -> Value;

    /// A tool with nothing to offer right now is not shown to the model.
    fn available(&self, _context: &ToolContext<'_>) -> bool {
        true
    }

    /// An error is shown to the model as the result, so it can correct itself.
    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> ToolFuture<'a>;
}
