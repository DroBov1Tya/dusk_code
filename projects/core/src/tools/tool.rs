//! The tool interface: what the model reads about a tool, and the call that turns JSON
//! arguments into text for the model.

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::sync::mpsc;

#[cfg(target_os = "macos")]
use super::approvals::WindowAccess;
use super::{
    Shaped,
    approvals::{Approvals, Request},
    scope::{Place, ToolScope},
};
use crate::{
    BoxFuture,
    config::{ApprovalPolicy, Config},
    platform::Needs,
    runtime::ThreadEvent,
};

pub use dusk_protocol::config::ToolKind;

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
    /// The thread's record, where its own approval mode lives; `None` where no runtime
    /// runs the tools.
    pub state: Option<Arc<crate::state::State>>,
    /// The thread the tool acts for, with `state`.
    pub thread_id: String,
}

impl ToolContext {
    /// The policy that decides a request: the thread's own mode when it has one, the
    /// default in the settings otherwise. A failed read falls back to the settings.
    pub async fn approvals_policy(&self) -> ApprovalPolicy {
        let Some(state) = &self.state else {
            return self.config.approvals();
        };
        match state.thread_approvals(&self.thread_id).await {
            Ok(Some(policy)) => policy,
            Ok(None) => self.config.approvals(),
            Err(error) => {
                tracing::warn!("cannot read the thread's approval mode: {error:#}");
                self.config.approvals()
            }
        }
    }

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
            let policy = self.approvals_policy().await;
            approvals
                .ask(&self.thread_id, request, self.progress.clone(), policy)
                .await?;
        }
        Ok(resolved)
    }

    /// Screen actions, asked as one request: see [`Approvals::ask_all`]. The screen needs
    /// the user's yes once a session, the keyboard every time, unless full access gives
    /// it. A screen grant never permits the system pointer or a window, and without an
    /// approval service only the explicitly selected full-access policy allows those.
    pub async fn approve_computer(&self, requests: Vec<Request>) -> Result<()> {
        let Some(approvals) = &self.approvals else {
            let guarded = requests.iter().any(|request| {
                matches!(
                    request,
                    Request::SystemPointer { .. } | Request::Window { .. }
                )
            });
            anyhow::ensure!(
                !guarded || self.approvals_policy().await == ApprovalPolicy::FullAccess,
                "system pointer control and window use need an approval service or explicit \
                 full access"
            );
            return Ok(());
        };
        let policy = self.approvals_policy().await;
        approvals
            .ask_all(&self.thread_id, requests, self.progress.clone(), policy)
            .await
    }

    /// Capture and activation are separate approvals on one retained window.
    /// Ending confinement does not inherit either session grant. Only `window_session`
    /// asks this way, and only macOS has it.
    #[cfg(target_os = "macos")]
    pub async fn approve_window(&self, grant: Option<WindowAccess>, action: &str) -> Result<()> {
        let Some(approvals) = &self.approvals else {
            anyhow::ensure!(
                self.approvals_policy().await == ApprovalPolicy::FullAccess,
                "window use needs an approval service or explicit full access"
            );
            return Ok(());
        };
        let policy = self.approvals_policy().await;
        approvals
            .ask(
                &self.thread_id,
                Request::Window {
                    grant,
                    action: action.to_owned(),
                },
                self.progress.clone(),
                policy,
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
        let policy = self.approvals_policy().await;
        approvals
            .ask(&self.thread_id, request, self.progress.clone(), policy)
            .await
    }
}

/// The sources of chats nobody who may change the settings is there to answer: the
/// timetable's runs, a headless `duskcode run` (`cli`) and the evals' tasks.
pub(crate) const UNATTENDED: [&str; 3] = ["schedule", "cli", "evals"];

impl ToolContext {
    /// Asks the owner for a change of the settings, in every mode (`Approvals::ask_owner`).
    /// Refused at once where the owner cannot answer: no approvals here, a chat the
    /// timetable or a headless run started, or one an API token started, whose client
    /// may not answer it; the call would otherwise wait for good.
    pub async fn ask_owner(&self, request: Request) -> Result<()> {
        let nobody = "needs the owner's yes, and nobody who may give it can answer in this \
                      chat; ask the user to do it in the app";
        let approvals = self
            .approvals
            .as_ref()
            .with_context(|| format!("{} {nobody}", request.what()))?;
        let state = self
            .state
            .as_ref()
            .with_context(|| format!("{} {nobody}", request.what()))?;
        let chat = state.root_thread(&self.thread_id).await?;
        let thread = state
            .load_thread(&chat)
            .await?
            .with_context(|| format!("no chat {chat}"))?;
        anyhow::ensure!(
            thread.owner.is_none() && !UNATTENDED.contains(&thread.source.as_str()),
            "{} {nobody}",
            request.what()
        );
        approvals
            .ask_owner(&self.thread_id, request, self.progress.clone())
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

    /// The schema the model is shown. It differs from `parameters`, which a call is
    /// checked against, only for a tool whose arguments depend on a setting.
    fn offered_parameters(&self, _context: &ToolContext) -> Value {
        self.parameters()
    }

    /// A tool with nothing to offer right now is not shown to the model.
    fn available(&self, _context: &ToolContext) -> bool {
        true
    }

    /// The tools this call runs inside it, each with its own arguments, so switches and
    /// plugin hooks judge every one as if it were called alone. An error rejects the call.
    fn calls_within(&self, _arguments: &Value) -> Result<Vec<(String, Value)>> {
        Ok(Vec::new())
    }

    /// Running the call again after its result was lost changes nothing the first run
    /// did not: it only reads, or sets the same state again.
    fn repeatable(&self, _arguments: &Value) -> bool {
        false
    }

    /// The call changes nothing anywhere (no file, no state, no process, nothing shown to
    /// the user) and does not depend on another call of the same step, so it may run
    /// together with the read-only calls next to it. Stricter than `repeatable`: a memory
    /// rewrite is repeatable but not read-only. A read-only call must be repeatable, since
    /// a restart runs the open calls of such a group again.
    fn read_only(&self, _arguments: &Value) -> bool {
        false
    }

    /// The result as the model is to be sent it when that is shorter than what is
    /// stored (`limits.shape_output`); `None` sends it whole. Decided from the call and
    /// its result alone, so the same call always gives the same text.
    fn shape(&self, _arguments: &Value, _result: &super::registry::ToolResult) -> Option<Shaped> {
        None
    }

    /// A line added after the result that names a narrower command to run next, such as
    /// the failing tests alone (`limits.rerun_hint`). Decided from the call and its result
    /// alone, so the same call always gives the same line.
    fn rerun(&self, _arguments: &Value, _result: &super::registry::ToolResult) -> Option<String> {
        None
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

/// Control characters in names or lines would reach the model and terminals as markup.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() && c != '\t' { '?' } else { c })
        .collect()
}
