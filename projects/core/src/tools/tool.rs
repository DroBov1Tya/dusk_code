//! The tool interface: what the model reads about a tool, and the call that turns JSON
//! arguments into text for the model.

use std::{future::Future, pin::Pin};

use anyhow::Result;
use serde_json::Value;

use super::scope::ToolScope;

/// Boxed so tools of different kinds, built-in or MCP, fit in one registry.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;

pub trait Tool: Send + Sync {
    /// Unique across the registry; what the model calls.
    fn name(&self) -> &'static str;

    /// Written for the model: what the tool does and when to use it. The scope lets it
    /// name the real directories.
    fn description(&self, scope: &ToolScope) -> String;

    /// JSON Schema of the arguments object.
    fn parameters(&self) -> Value;

    /// An error is shown to the model as the result, so it can correct itself.
    fn call<'a>(&'a self, arguments: Value, scope: &'a ToolScope) -> ToolFuture<'a>;
}
