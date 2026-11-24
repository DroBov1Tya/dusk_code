//! `project_graph`: the project's files with their line counts, and what agents wrote
//! about them, for an agent to ask before it reads code. The core lists, counts, hashes
//! and stores (`project::graph`, `state::graph`); understanding code is the agent's, so
//! every summary, node and edge is one an agent recorded or the user corrected. The name
//! is kept from the code graph this grew out of, so saved configs that switch it apply.
//!
//! `map` works in any folder, by names alone when the folder is not a saved project;
//! every other action needs a saved project, where the graph is stored.

mod answer;
mod map;
mod record;
#[cfg(test)]
mod tests;
mod text;

use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    BoxFuture,
    project::{episodes::saved_dir, graph::refresh_within},
    state::State,
    tools::{
        console::skipped_folders,
        schema::{self, parse_arguments},
        tool::{Tool, ToolContext, ToolKind},
    },
};

/// A read refreshes the graph first unless a refresh started this recently.
const FRESH_ENOUGH: Duration = Duration::from_secs(5);
/// How long a read may wait for files to be listed and read.
const READ_BUDGET: Duration = Duration::from_secs(3);

#[derive(Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
enum Action {
    #[default]
    Map,
    About,
    Find,
    Impact,
    Record,
    Fix,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum NodeKindArgument {
    Module,
    Type,
    Function,
    Service,
    Test,
    Config,
    Doc,
    Other,
}

#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum EdgeKindArgument {
    Uses,
    Imports,
    Calls,
    Covers,
    Configures,
    Implements,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NodeArgument {
    name: String,
    kind: NodeKindArgument,
    /// First line, from 1.
    line: Option<u32>,
    end_line: Option<u32>,
    /// What it does, at most 200 characters.
    summary: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EdgeArgument {
    /// A node of this file by name; the file itself when left out.
    from: Option<String>,
    /// A node name, `path#name` or a file path.
    to: String,
    kind: EdgeKindArgument,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    /// `map` when left out.
    action: Option<Action>,
    /// map: a folder; about, record, fix: a file.
    path: Option<String>,
    /// map: part of a path or a glob such as `*.rs`; find: text of names, kinds, paths
    /// and summaries.
    find: Option<String>,
    /// about: a node name (`path` picks its file); fix: the node to change.
    node: Option<String>,
    /// fix: the kind of `node`, when the file has that name twice.
    kind: Option<NodeKindArgument>,
    /// impact: the files a change touches.
    files: Option<Vec<String>>,
    /// record: the file's responsibility in one or two sentences, at most 400
    /// characters; fix: the right text of the file's or `node`'s summary.
    summary: Option<String>,
    /// record: the main functions, types and so on the file holds.
    nodes: Option<Vec<NodeArgument>>,
    /// record: what the file or its nodes use, import or call.
    edges: Option<Vec<EdgeArgument>>,
    /// fix: the edge to delete.
    edge: Option<EdgeArgument>,
    /// fix: delete the summary, `node` or `edge` instead of changing it.
    delete: Option<bool>,
}

pub struct ProjectGraph;

impl Tool for ProjectGraph {
    fn name(&self) -> &'static str {
        "project_graph"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Files
    }

    fn description(&self, _context: &ToolContext) -> String {
        "The project graph: this project's files with line counts, and what agents recorded \
         about them (a file's summary, its nodes such as functions and types, edges such \
         as calls and imports). On a project you do not know, start here before reading \
         code. Actions: `map` (default) a tree with lines per file and folder and summaries, \
         deep folders folded; `path` opens a folder, `find` filters paths by text or glob. \
         `about` a file (`path`) or a node (`node`): summary, nodes, edges out and in. \
         `find` names, kinds, paths and summaries holding the text. `impact` files within \
         two edges of `files`. `record`: when you read a file whose summary is missing or \
         marked (stale), write `summary` (its responsibility in one or two sentences), \
         `nodes` (its main functions and types) and `edges` (what it calls, imports, uses). \
         `fix`: when the user says an entry is wrong, give the right `summary` of the file \
         or a `node`, or `delete` it or an `edge`. Facts are notes, not the code: (stale) \
         means the file changed since."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn available(&self, context: &ToolContext) -> bool {
        context.config.limits().project_graph
    }

    fn repeatable(&self, arguments: &Value) -> bool {
        reads_only(arguments)
    }

    fn read_only(&self, arguments: &Value) -> bool {
        reads_only(arguments)
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: Arguments = parse_arguments(arguments)?;
            let action = arguments.action.unwrap_or_default();
            let graph = Graph::of(&context).await?;
            match action {
                Action::Map => map::answer(&context, graph.as_ref(), &arguments).await,
                Action::About => answer::about(&context, &needed(graph)?, &arguments).await,
                Action::Find => answer::find(&needed(graph)?, &arguments).await,
                Action::Impact => answer::impact(&context, &needed(graph)?, &arguments).await,
                Action::Record => record::record(&context, &needed(graph)?, arguments).await,
                Action::Fix => record::fix(&context, &needed(graph)?, arguments).await,
            }
        })
    }
}

/// Whether the call only reads: every action but `record` and `fix`.
fn reads_only(arguments: &Value) -> bool {
    !matches!(
        arguments.get("action").and_then(Value::as_str),
        Some("record" | "fix")
    )
}

/// The stored graph of the thread's folder, refreshed when it is not fresh.
struct Graph<'a> {
    state: &'a State,
    project: String,
    /// New or changed files the refresh left unread.
    pending: usize,
}

impl<'a> Graph<'a> {
    /// `None` when the folder is not a saved project or no store runs the tools.
    async fn of(context: &'a ToolContext) -> Result<Option<Self>> {
        let Some(state) = context.state.as_deref() else {
            return Ok(None);
        };
        let dir = context.scope.cwd();
        let Some(project) = saved_dir(state, dir).await? else {
            return Ok(None);
        };
        let skip: Vec<PathBuf> = skipped_folders(&context.scope);
        let refreshed = refresh_within(state, dir, &skip, FRESH_ENOUGH, READ_BUDGET)
            .await
            .context("refresh the project graph")?;
        Ok(Some(Self {
            state,
            project,
            pending: refreshed.map_or(0, |refreshed| refreshed.pending),
        }))
    }
}

fn needed(graph: Option<Graph<'_>>) -> Result<Graph<'_>> {
    match graph {
        Some(graph) => Ok(graph),
        None => bail!(
            "this folder is not a saved project, so it has no graph; `map` still lists its \
             files"
        ),
    }
}
