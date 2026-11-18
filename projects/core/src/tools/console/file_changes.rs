//! What a foreground command changed in its project, for the change ledger and the
//! chat's list of changed files: one detector for both. The project is walked before
//! and after the command; see `project::ledger`.
//!
//! Background jobs are not watched: other commands and the user change files while a
//! job runs, so a walk around it could not tell whose change it saw.

use std::path::{Path, PathBuf};

use dusk_protocol::project::{Change, ChangeKind, EditKind};

use crate::project::graph::{Changed, Folder};
use crate::project::ledger::{self, Edit, Snapshot};
use crate::tools::{ToolScope, tool::ToolContext};

/// The project as it was before a command.
pub struct Watch {
    root: PathBuf,
    before: Snapshot,
}

/// `None` when nothing is recorded: no state, recording switched off, or a project past
/// the walk's caps.
pub async fn watch(context: &ToolContext) -> Option<Watch> {
    context.state.as_ref()?;
    if !context.config.limits().record_changes {
        return None;
    }
    let root = context.scope.cwd().to_owned();
    let skip = skipped(&context.scope);
    let walked = root.clone();
    match tokio::task::spawn_blocking(move || ledger::take(&walked, &skip)).await {
        Ok(Some(before)) => Some(Watch { root, before }),
        Ok(None) => {
            tracing::info!("the project is too large to record what a command changes");
            unrecorded(context);
            None
        }
        Err(error) => {
            tracing::warn!("cannot walk the project before a command: {error}");
            None
        }
    }
}

impl Watch {
    /// Walks the project again and records what changed against the walk before; files
    /// whose name starts with `marker` are the command's own. A failure is logged: the
    /// command's own result stands.
    pub async fn record(self, context: &ToolContext, marker: &str) {
        let Some(state) = &context.state else {
            return;
        };
        let skip = skipped(&context.scope);
        let Self { root, before } = self;
        let walked = root.clone();
        let found = tokio::task::spawn_blocking(move || {
            let after = ledger::take(&walked, &skip)?;
            ledger::compare(&walked, &before, &after)
        })
        .await;
        let mut edits = match found {
            Ok(Some(edits)) => edits,
            Ok(None) => {
                tracing::info!(
                    "a command changed too much, or the project is too large, to record"
                );
                unrecorded(context);
                return;
            }
            Err(error) => {
                tracing::warn!("cannot compare the project after a command: {error}");
                return;
            }
        };
        // The marker shows only when the workspace is the project itself.
        edits.retain(|edit| !file_name(&edit.path).starts_with(marker));
        if edits.is_empty() {
            return;
        }
        let call_id = context.progress.as_ref().map(|p| p.call_id.as_str());
        let project = root.to_string_lossy();
        // Paths and kinds only: a diff may quote a secret, so it is never logged.
        if let Err(error) = state
            .record_changes(&context.thread_id, call_id, &project, &edits)
            .await
        {
            tracing::warn!("cannot record the changes a command made: {error:#}");
        }
        // The graph follows the same edits: those files only, by the hash taken here.
        if let Some(graph) = &context.graph {
            let changed = edits.iter().map(|edit| Changed {
                path: edit.path.clone(),
                hash: edit.hash_after.clone(),
            });
            graph.changed(project_folder(&context.scope), changed.collect());
        }
        let touched: Vec<Change> = edits.iter().map(touched).collect();
        if let Err(error) = state.note_thread_files(&context.thread_id, &touched).await {
            tracing::warn!("cannot note the files a command changed: {error:#}");
        }
    }
}

/// The config folder and the thread's workspace when they lie inside the project: the
/// one holds the session database, the other the thread's own scratch files.
fn skipped(scope: &ToolScope) -> Vec<PathBuf> {
    let root = scope.cwd();
    scope
        .protected()
        .into_iter()
        .chain([scope.workspace()])
        .filter(|folder| *folder != root && folder.starts_with(root))
        .map(Path::to_owned)
        .collect()
}

/// The thread's project as the graph's indexer walks it: what the ledger walks.
pub fn project_folder(scope: &ToolScope) -> Folder {
    Folder {
        root: scope.cwd().to_owned(),
        skip: skipped(scope),
    }
}

/// The ledger cannot say what the command changed, so the graph looks for itself when
/// the turn ends.
fn unrecorded(context: &ToolContext) {
    if let Some(graph) = &context.graph {
        graph.unrecorded(context.scope.cwd());
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn touched(edit: &Edit) -> Change {
    Change {
        path: edit.path.clone(),
        kind: match edit.kind {
            EditKind::Created => ChangeKind::Added,
            EditKind::Modified => ChangeKind::Modified,
            EditKind::Deleted => ChangeKind::Deleted,
        },
    }
}
