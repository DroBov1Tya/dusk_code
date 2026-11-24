//! `record` and `fix`: an agent's facts about one file, and the user's corrections. The
//! input is the model's and untrusted: the file must be a regular file inside the
//! project that the core hashed (no secrets, not too large), a team member writes only
//! about the files of its task, every text is made plain and cleared of secrets, and
//! the store checks kinds, sizes and counts again. The file is measured now, so each
//! fact is tied to the version the agent read.

use std::{path::Path, time::Duration};

use anyhow::{Context, Result, bail, ensure};

use super::{
    Arguments, EdgeArgument, EdgeKindArgument, Graph, NodeKindArgument, READ_BUDGET, map::inside,
};
use crate::{
    project::{
        episodes::forwarded,
        graph::{MAX_READ_BYTES, measure, refresh_within},
        ledger::may_keep_text,
    },
    runtime::team::is_member,
    state::{
        graph::{Author, EdgeKind, FixTarget, NewEdge, NewNode, NodeKind, Record},
        overlaps,
    },
    tools::{console::skipped_folders, tool::ToolContext},
};

/// Nodes and edges one `record` takes.
const MAX_NODES: usize = 64;
const MAX_EDGES: usize = 128;
/// Longest summary taken: a file's; the store checks a node's own cap.
const MAX_SUMMARY: usize = 400;

pub(super) async fn record(
    context: &ToolContext,
    graph: &Graph<'_>,
    arguments: Arguments,
) -> Result<String> {
    let path = file_of(context, arguments.path.as_deref()).await?;
    may_write(context, graph, &path).await?;
    let nodes = arguments.nodes.unwrap_or_default();
    let edges = arguments.edges.unwrap_or_default();
    ensure!(
        arguments.summary.is_some() || !nodes.is_empty() || !edges.is_empty(),
        "give `summary`, `nodes` or `edges` to record"
    );
    ensure!(nodes.len() <= MAX_NODES, "at most {MAX_NODES} nodes a call");
    ensure!(edges.len() <= MAX_EDGES, "at most {MAX_EDGES} edges a call");
    let dir = context.scope.cwd();
    let mut record = Record {
        summary: match arguments.summary.as_deref() {
            Some(summary) => Some(cleaned(graph, dir, summary).await?),
            None => None,
        },
        ..Record::default()
    };
    for node in nodes {
        record.nodes.push(NewNode {
            name: node.name.trim().to_owned(),
            kind: node_kind(node.kind),
            line: node.line,
            end_line: node.end_line,
            summary: match node.summary.as_deref() {
                Some(summary) => cleaned(graph, dir, summary).await?,
                None => String::new(),
            },
        });
    }
    record.edges = edges.into_iter().map(new_edge).collect();
    let hash = measured(context, graph, &path).await?;
    let done = graph
        .state
        .graph_record(
            &graph.project,
            &path,
            &hash,
            &record,
            author(context, false),
        )
        .await?;
    let mut parts = Vec::new();
    if record.summary.is_some() {
        parts.push("summary".to_owned());
    }
    if done.nodes > 0 {
        parts.push(format!("{} nodes", done.nodes));
    }
    if done.edges > 0 {
        parts.push(format!("{} edges", done.edges));
    }
    let mut answer = format!("recorded `{path}`: {}", parts.join(", "));
    if done.replaced > 0 {
        answer.push_str(&format!("; replaced {} stale", done.replaced));
    }
    if done.kept > 0 {
        answer.push_str(&format!(
            "; kept {} the user corrected (use `fix` if the user asks)",
            done.kept
        ));
    }
    Ok(answer)
}

pub(super) async fn fix(
    context: &ToolContext,
    graph: &Graph<'_>,
    arguments: Arguments,
) -> Result<String> {
    let path = file_of(context, arguments.path.as_deref()).await?;
    may_write(context, graph, &path).await?;
    let delete = arguments.delete.unwrap_or(false);
    let target = match (arguments.node, arguments.edge) {
        (Some(_), Some(_)) => bail!("give `node` or `edge`, not both"),
        (Some(name), None) => FixTarget::Node {
            name: name.trim().to_owned(),
            kind: arguments.kind.map(node_kind),
        },
        (None, Some(edge)) => {
            let edge = new_edge(edge);
            FixTarget::Edge {
                from: edge.from,
                to: edge.to,
                kind: edge.kind,
            }
        }
        (None, None) => FixTarget::Summary,
    };
    let text = match (delete, arguments.summary.as_deref()) {
        (true, None) => None,
        (true, Some(_)) => bail!("give `summary` or `delete`, not both"),
        (false, Some(summary)) => Some(cleaned(graph, context.scope.cwd(), summary).await?),
        (false, None) => bail!("give the right `summary`, or `delete`"),
    };
    let hash = measured(context, graph, &path).await?;
    let fixed = graph
        .state
        .graph_fix(
            &graph.project,
            &path,
            &hash,
            &target,
            text.as_deref(),
            author(context, true),
        )
        .await?;
    ensure!(fixed, "no such entry of `{path}`; see `about`");
    let what = match &target {
        FixTarget::Summary => "the summary".to_owned(),
        FixTarget::Node { name, .. } => format!("node `{name}`"),
        FixTarget::Edge { to, kind, .. } => format!("edge {} `{to}`", kind.as_str()),
    };
    let done = if text.is_some() { "fixed" } else { "deleted" };
    Ok(format!("{done} {what} of `{path}`, marked as the user's"))
}

/// `path` as a file of the project, relative with `/`, that the graph may hold facts
/// about.
async fn file_of(context: &ToolContext, path: Option<&str>) -> Result<String> {
    let path = path.context("give `path` of a file")?;
    let path = inside(&context.scope, path)
        .await?
        .context("give a file as `path`, not the project")?;
    ensure!(
        may_keep_text(&path),
        "the graph keeps no facts about files that hold secrets"
    );
    let meta = tokio::fs::symlink_metadata(context.scope.cwd().join(&path))
        .await
        .with_context(|| format!("no file `{path}` in the project"))?;
    ensure!(meta.is_file(), "`{path}` is not a file");
    ensure!(
        meta.len() <= MAX_READ_BYTES,
        "`{path}` is too large for the graph to keep facts about"
    );
    Ok(path)
}

/// A team member writes only about the files of the task it holds.
async fn may_write(context: &ToolContext, graph: &Graph<'_>, path: &str) -> Result<()> {
    if context.thread_id.is_empty() {
        return Ok(());
    }
    let Some(thread) = graph.state.load_thread(&context.thread_id).await? else {
        return Ok(());
    };
    if !is_member(&thread) {
        return Ok(());
    }
    let files = graph
        .state
        .held_task_files(&context.thread_id)
        .await
        .context("read the task's files")?
        .context("a team member records facts only while it holds a task")?;
    ensure!(
        files.entries().iter().any(|entry| overlaps(entry, path)),
        "`{path}` is not among this task's files, so a member cannot write about it"
    );
    Ok(())
}

/// Measures the file as it is now and stores that; its hash, which the facts are
/// written against. A file the graph does not list (ignored, or so new that no refresh
/// saw it yet) is looked for once more: facts about a file the next refresh drops would
/// be lost without a word.
async fn measured(context: &ToolContext, graph: &Graph<'_>, path: &str) -> Result<String> {
    let state = graph.state;
    if state.graph_file(&graph.project, path).await?.is_none() {
        let skip = skipped_folders(&context.scope);
        refresh_within(
            state,
            context.scope.cwd(),
            &skip,
            Duration::ZERO,
            READ_BUDGET,
        )
        .await?;
        ensure!(
            state.graph_file(&graph.project, path).await?.is_some(),
            "`{path}` is not in the project graph: ignored files are left out"
        );
    }
    let root = context.scope.cwd().to_owned();
    let relative = path.to_owned();
    let file = tokio::task::spawn_blocking(move || measure(&root, &relative))
        .await
        .context("the reading of the file stopped")?
        .with_context(|| format!("cannot read `{path}`"))?;
    ensure!(
        !file.hash.is_empty(),
        "the graph keeps no facts about `{path}`"
    );
    let hash = file.hash.clone();
    graph
        .state
        .graph_update(&graph.project, &[file], &[])
        .await?;
    Ok(hash)
}

/// `text` on one line, cleared of the project's secrets and what looks like one, cut to
/// the cap.
async fn cleaned(graph: &Graph<'_>, dir: &Path, text: &str) -> Result<String> {
    ensure!(
        text.chars().count() <= MAX_SUMMARY,
        "a summary is at most {MAX_SUMMARY} characters"
    );
    let one_line = text.replace(['\n', '\r'], " ");
    let clean = forwarded(graph.state, dir, &one_line).await?;
    Ok(clean.chars().take(MAX_SUMMARY).collect())
}

fn author(context: &ToolContext, by_user: bool) -> Author<'_> {
    Author {
        thread: (!context.thread_id.is_empty()).then_some(context.thread_id.as_str()),
        by_user,
    }
}

fn new_edge(edge: EdgeArgument) -> NewEdge {
    NewEdge {
        from: edge
            .from
            .map(|from| from.trim().to_owned())
            .filter(|from| !from.is_empty()),
        to: edge.to.trim().to_owned(),
        kind: match edge.kind {
            EdgeKindArgument::Uses => EdgeKind::Uses,
            EdgeKindArgument::Imports => EdgeKind::Imports,
            EdgeKindArgument::Calls => EdgeKind::Calls,
            EdgeKindArgument::Covers => EdgeKind::Covers,
            EdgeKindArgument::Configures => EdgeKind::Configures,
            EdgeKindArgument::Implements => EdgeKind::Implements,
        },
    }
}

fn node_kind(kind: NodeKindArgument) -> NodeKind {
    match kind {
        NodeKindArgument::Module => NodeKind::Module,
        NodeKindArgument::Type => NodeKind::Type,
        NodeKindArgument::Function => NodeKind::Function,
        NodeKindArgument::Service => NodeKind::Service,
        NodeKindArgument::Test => NodeKind::Test,
        NodeKindArgument::Config => NodeKind::Config,
        NodeKindArgument::Doc => NodeKind::Doc,
        NodeKindArgument::Other => NodeKind::Other,
    }
}
