//! The tool interface: what the model reads about a tool, and the call that turns JSON
//! arguments into text for the model.

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::sync::mpsc;

use super::{
    approvals::{Approvals, Request},
    scope::{Place, ToolScope},
};
use crate::{BoxFuture, config::Config, platform::Needs, runtime::ThreadEvent};

/// What a tool sees: where it may act and the settings of the current step.
/// Owned, so a call can keep it across awaits and hand it on without lifetimes.
#[derive(Clone)]
pub struct ToolContext {
    pub scope: Arc<ToolScope>,
    pub config: Arc<Config>,
    /// The running call; `None` while tools are only described to the model.
    pub progress: Option<Progress>,
    /// Where requests for the user's approval go; `None` where no runtime runs the tools.
    pub approvals: Option<Arc<Approvals>>,
}

impl ToolContext {
    /// Resolves a path the model gave; one outside the folders needs the user's yes.
    pub async fn resolve(&self, path: &str, write: bool) -> Result<PathBuf> {
        let (resolved, place) = self.scope.locate(path)?;
        if place == Place::Outside {
            let approvals = self
                .approvals
                .as_ref()
                .context("path is outside the working directory and the thread workspace")?;
            let request = Request::Path {
                path: resolved.clone(),
                write,
            };
            approvals
                .ask(request, self.progress.clone(), self.config.approvals())
                .await?;
        }
        Ok(resolved)
    }

    /// The screen needs the user's yes once a session, the keyboard every time, unless
    /// full access gives it.
    pub async fn approve_screen(&self, action: &str, keyboard: bool) -> Result<()> {
        let Some(approvals) = &self.approvals else {
            return Ok(());
        };
        let action = action.to_owned();
        let request = match keyboard {
            true => Request::Keyboard { action },
            false => Request::Screen { action },
        };
        approvals
            .ask(request, self.progress.clone(), self.config.approvals())
            .await
    }

    /// Every command needs the user's yes, unless full access or the session gives it.
    pub async fn approve_command(&self, command: &str) -> Result<()> {
        let Some(approvals) = &self.approvals else {
            return Ok(());
        };
        let request = Request::Command {
            command: command.to_owned(),
        };
        approvals
            .ask(request, self.progress.clone(), self.config.approvals())
            .await
    }
}

/// Where a running call reports what happens inside it, such as a subagent at work.
#[derive(Clone)]
pub struct Progress {
    pub call_id: String,
    pub events: mpsc::UnboundedSender<ThreadEvent>,
}

impl Progress {
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
    /// The screen: screenshots, mouse and keyboard. Heavy, so off until switched on.
    Computer,
}

impl ToolKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Console => "console",
            Self::Agents => "agents",
            Self::Harness => "harness",
            Self::Mcp => "mcp",
            Self::Computer => "computer",
        }
    }

    /// Whether a tool of this kind is on before the user switches it.
    pub fn on_by_default(self) -> bool {
        self != Self::Computer
    }
}

pub trait Tool: Send + Sync {
    /// Unique across the registry; what the model calls.
    fn name(&self) -> &str;

    /// Trusted launcher depth decides whether this tool may be offered or executed.
    fn root_only(&self) -> bool {
        false
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Harness
    }

    /// What the machine must offer; a tool it lacks is not offered nor switchable.
    fn needs(&self) -> Needs {
        Needs::Nothing
    }

    /// A tool with a setting of its own, such as memory, is not among the tool switches.
    fn switchable(&self) -> bool {
        true
    }

    /// Written for the model: what the tool does and when to use it. The context lets it
    /// name the real directories.
    fn description(&self, context: &ToolContext) -> String;

    /// JSON Schema of the arguments object.
    fn parameters(&self) -> Value;

    /// A tool with nothing to offer right now is not shown to the model.
    fn available(&self, _context: &ToolContext) -> bool {
        true
    }

    /// An error is shown to the model as the result, so it can correct itself.
    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String>;

    /// Typed attachments stay outside the text the provider receives. Existing tools
    /// need only implement the text call above.
    fn call_result<'a>(
        &'a self,
        arguments: Value,
        context: ToolContext,
    ) -> BoxFuture<'a, super::registry::ToolResult> {
        Box::pin(async move {
            let content = self.call(arguments, context).await?;
            Ok(super::registry::ToolResult::text(content))
        })
    }
}
