//! The tool interface: what the model reads about a tool, and the call that turns JSON
//! arguments into text for the model.

use std::{future::Future, pin::Pin};

use anyhow::Result;
use serde_json::Value;
use tokio::sync::mpsc;

use super::scope::ToolScope;
use crate::{config::Config, runtime::ThreadEvent};

/// Boxed so tools of different kinds, built-in or MCP, fit in one registry.
pub type BoxFuture<'a, String> = Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;

/// What a tool sees: where it may act and the settings of the current step.
#[derive(Clone, Copy)]
pub struct ToolContext<'a> {
    pub scope: &'a ToolScope,
    pub config: &'a Config,
    /// The running call; `None` while tools are only described to the model.
    pub progress: Option<Progress<'a>>,
}

/// Where a running call reports what happens inside it, such as a subagent at work.
#[derive(Clone, Copy)]
pub struct Progress<'a> {
    pub call_id: &'a str,
    pub events: &'a mpsc::UnboundedSender<ThreadEvent>,
}

impl Progress<'_> {
    pub fn emit(&self, event: ThreadEvent) {
        // A client that stopped listening does not stop the call.
        let _ = self.events.send(event);
    }
}

/// What a tool works on, for clients to group tools by; in the order they are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToolKind {
    Files,
    Console,
    Agents,
    /// The thread itself: its history, tasks and skills.
    Harness,
    Mcp,
}

impl ToolKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Console => "console",
            Self::Agents => "agents",
            Self::Harness => "harness",
            Self::Mcp => "mcp",
        }
    }
}

pub trait Tool: Send + Sync {
    /// Unique across the registry; what the model calls.
    fn name(&self) -> &str;

    fn kind(&self) -> ToolKind {
        ToolKind::Harness
    }

    /// A tool with a setting of its own, such as memory, is not among the tool switches.
    fn switchable(&self) -> bool {
        true
    }

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
    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String>;
}
