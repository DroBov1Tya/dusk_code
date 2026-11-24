//! `branch`: an agent works on a git branch of its own, in a worktree beside the
//! repository, and merges it back when its checks pass. `work` does each step through
//! `project::git`; `follow` moves the thread's tools to the worktree and back.

mod follow;
#[cfg(test)]
mod tests;
mod work;

pub use follow::scope_for;
pub use work::start;

use anyhow::{Context, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    BoxFuture,
    runtime::ThreadEvent,
    tools::{Tool, ToolContext, ToolKind, schema, schema::parse_arguments},
};

pub struct Branch;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    action: Action,
    /// For start: a short name; the branch is `agent/<name>`.
    name: Option<String>,
    /// For commit: the commit message.
    message: Option<String>,
    /// An agent you asked with `isolated`, by name: act on its branch instead of yours.
    agent: Option<String>,
}

#[derive(Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Action {
    Start,
    Status,
    Commit,
    Merge,
    Abort,
    Abandon,
    List,
}

impl Tool for Branch {
    fn name(&self) -> &'static str {
        "branch"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Console
    }

    fn description(&self, _: &ToolContext) -> String {
        "Work on a git branch of your own in a separate worktree, then merge it back. Use it \
         for risky or parallel changes. start: makes agent/<name> from the current branch \
         and a worktree beside the repository; from your next step your files and commands \
         are there. status: commits ahead and behind, uncommitted files, a waiting merge. \
         commit: commits every change in the worktree (also finishes a merge once its \
         conflicts are edited). merge: when the checks pass in the worktree, brings the base \
         branch in first (its conflicts are listed for you to resolve and commit, or \
         abort), then merges into the branch it came from with a merge commit, removes the \
         worktree and the merged branch; a merge into the user's branch waits for their \
         yes. abort: drops a waiting merge. abandon: removes the worktree; uncommitted work \
         is committed to the branch and a branch with work is kept. list: the branches you \
         and your agents work on. `agent` acts on the branch of an agent you asked with \
         `isolated`."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn repeatable(&self, arguments: &Value) -> bool {
        reads(arguments)
    }

    fn read_only(&self, arguments: &Value) -> bool {
        reads(arguments)
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: Arguments = parse_arguments(arguments)?;
            let state = context
                .state
                .clone()
                .context("branches need the thread's record, which is not here")?;
            let thread = context.thread_id.as_str();
            let agent = arguments.agent.as_deref();
            if matches!(arguments.action, Action::Start | Action::List) {
                ensure!(agent.is_none(), "`agent` is for a branch an agent works on");
            }
            match arguments.action {
                Action::Start => {
                    context
                        .approve_git("start a branch of its own in a new worktree")
                        .await?;
                    let name = arguments.name.as_deref();
                    let made = start(&state, thread, thread, &context.scope, name).await?;
                    tell(&context, Some(work::shown(&made)));
                    Ok(format!(
                        "Working on branch {} from {}. From your next step your files and \
                         commands are in {}; paths in {} are outside it now. Commit as you \
                         go, and merge when the checks pass here.",
                        made.branch, made.base, made.dir, made.worktree_of
                    ))
                }
                Action::Status => work::status(&work::find(&state, thread, agent).await?).await,
                Action::Commit => {
                    let message = arguments
                        .message
                        .as_deref()
                        .context("commit needs a message")?;
                    let worktree = work::find(&state, thread, agent).await?;
                    context
                        .approve_git(&format!("commit on {}: {message}", worktree.branch))
                        .await?;
                    work::commit(&worktree, message).await
                }
                Action::Merge => {
                    let worktree = work::find(&state, thread, agent).await?;
                    let done = work::merge(&context, &state, &worktree).await?;
                    Ok(finished(&context, &worktree, done))
                }
                Action::Abort => work::abort(&work::find(&state, thread, agent).await?).await,
                Action::Abandon => {
                    let worktree = work::find(&state, thread, agent).await?;
                    let done = work::abandon(&context, &state, &worktree).await?;
                    Ok(finished(&context, &worktree, done))
                }
                Action::List => work::list(&state, thread).await,
            }
        })
    }
}

/// The result, and the clients told when this thread is back in its own folder.
fn finished(
    context: &ToolContext,
    worktree: &crate::state::ThreadWorktree,
    done: work::Done,
) -> String {
    match done {
        work::Done::Finished(text) => {
            if worktree.thread_id == context.thread_id {
                tell(context, None);
            }
            text
        }
        work::Done::Waits(text) => text,
    }
}

/// The thread's branch changed: its clients show it.
fn tell(context: &ToolContext, branch: Option<dusk_protocol::project::ThreadBranch>) {
    if let Some(progress) = &context.progress {
        progress.emit(ThreadEvent::Branch(branch));
    }
}

fn reads(arguments: &Value) -> bool {
    matches!(
        arguments.get("action").and_then(Value::as_str),
        Some("status" | "list")
    )
}
