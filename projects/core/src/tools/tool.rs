//! The tool interface: what the model reads about a tool, and the call that turns JSON
//! arguments into text for the model.

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::sync::mpsc;

use super::{
    approvals::{Approvals, Request, WindowAccess},
    scope::{Place, ToolScope},
};
use crate::{BoxFuture, config::Config, platform::Needs, runtime::ThreadEvent};

pub use dusk_protocol::settings::ToolKind;

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

    /// A screen grant never permits physical pointer takeover. Without an approval
    /// service only the explicitly selected full-access policy allows it.
    pub async fn approve_system_pointer(&self, action: &str) -> Result<()> {
        let Some(approvals) = &self.approvals else {
            anyhow::ensure!(
                self.config.approvals() == crate::config::ApprovalPolicy::FullAccess,
                "system pointer control needs a separate approval service or explicit full access"
            );
            return Ok(());
        };
        approvals
            .ask(
                Request::SystemPointer {
                    action: action.to_owned(),
                },
                self.progress.clone(),
                self.config.approvals(),
            )
            .await
    }

    /// Capture and activation are separate approvals on one retained window.
    /// Ending confinement does not inherit either session grant.
    pub async fn approve_window(&self, grant: Option<WindowAccess>, action: &str) -> Result<()> {
        let Some(approvals) = &self.approvals else {
            anyhow::ensure!(
                self.config.approvals() == crate::config::ApprovalPolicy::FullAccess,
                "window use needs an approval service or explicit full access"
            );
            return Ok(());
        };
        approvals
            .ask(
                Request::Window {
                    grant,
                    action: action.to_owned(),
                },
                self.progress.clone(),
                self.config.approvals(),
            )
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
