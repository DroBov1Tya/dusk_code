//! `project_graph`: where a name is defined, who uses it, what depends on a file and which
//! tests cover it, answered from the project's index in one short result.
//! The questions are in
//! `project::graph::ask`; here are the arguments and the scope, and in `text` the wording.

#[cfg(test)]
mod tests;
mod text;
#[cfg(test)]
mod text_tests;

use anyhow::{Result, bail, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::changes::project_path;
use crate::{
    BoxFuture,
    project::graph::{
        Changed, PATHS_WAIT, Status,
        ask::{self, Asked, Question},
    },
    state::State,
    tools::{
        console::project_folder,
        schema::{self, parse_arguments},
        scope::ToolScope,
        tool::{Tool, ToolContext, ToolKind},
    },
};
use text::render;

/// Longest `name` taken, in characters.
const MAX_NAME_CHARS: usize = 256;
/// Longest `path` taken, in characters.
const MAX_PATH_CHARS: usize = 1024;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    ask: Ask,
    /// A symbol, `Type::member`, or a file path.
    name: String,
    /// Only under this folder.
    path: Option<String>,
}

// No doc comment: the schema the model reads carries none for `ask`.
#[derive(Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Ask {
    Where,
    Uses,
    Touches,
    Tests,
}

impl From<Ask> for Question {
    fn from(ask: Ask) -> Self {
        match ask {
            Ask::Where => Self::Where,
            Ask::Uses => Self::Uses,
            Ask::Touches => Self::Touches,
            Ask::Tests => Self::Tests,
        }
    }
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
        "Structure of this project from its index, without reading files. `ask`: `where` is \
         `name` defined; `uses` of it, as file:line; `touches`: files that depend on the file \
         `name`; `tests` that cover a file or symbol. Matched by name, not by type: a line \
         starting with `?` is a guess to check. Use this before search_files for definitions \
         and callers."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    /// Offered only once a refresh of this project finished in this run of the core: a
    /// read of memory, no query.
    fn available(&self, context: &ToolContext) -> bool {
        context
            .graph
            .as_ref()
            .is_some_and(|graph| matches!(graph.status(context.scope.cwd()), Status::Ready(_)))
    }

    fn repeatable(&self, _arguments: &Value) -> bool {
        true
    }

    fn read_only(&self, _arguments: &Value) -> bool {
        true
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let Arguments { ask, name, path } = parse_arguments(arguments)?;
            ensure!(
                name.chars().count() <= MAX_NAME_CHARS,
                "`name` is longer than {MAX_NAME_CHARS} characters"
            );
            let (Some(state), Some(graph)) = (&context.state, &context.graph) else {
                bail!("there is no project graph here; use search_files");
            };
            let folder = project_folder(&context.scope);
            let Status::Ready(counts) = graph.status(&folder.root) else {
                bail!("the project graph is not ready; use search_files");
            };
            let filter = match path.as_deref() {
                Some(path) => folder_filter(&context.scope, path).await?,
                None => None,
            };
            // A file this turn changed is answered as it is now, when its job is quick.
            graph.settle(&folder.root, PATHS_WAIT).await;
            let asked = Asked {
                question: ask.into(),
                name: &name,
                folder: filter.as_deref(),
            };
            let (text, changed) = answer(state, &folder.root, counts.files, asked).await?;
            let changed: Vec<Changed> = changed
                .into_iter()
                .map(|path| Changed { path, hash: None })
                .collect();
            graph.changed(folder, changed);
            Ok(text)
        })
    }
}

/// The answer as the model reads it, and the files of it that changed since they were
/// indexed, sorted.
pub(super) async fn answer(
    state: &State,
    root: &std::path::Path,
    files: u64,
    asked: Asked<'_>,
) -> Result<(String, Vec<String>)> {
    let reply = ask::ask(state, root, files, asked).await?;
    let mut changed: Vec<String> = reply.changed.iter().cloned().collect();
    changed.sort();
    Ok((render(&reply), changed))
}

/// `path` as a folder of the project, relative with `/`; `None` for the whole project.
/// An absolute path is taken when it is inside the project; then it passes the checks
/// of the other file tools: no `..`, no link that leads out.
async fn folder_filter(scope: &ToolScope, path: &str) -> Result<Option<String>> {
    ensure!(
        path.chars().count() <= MAX_PATH_CHARS,
        "`path` is longer than {MAX_PATH_CHARS} characters"
    );
    let trimmed = path.trim();
    let relative = std::path::Path::new(trimmed)
        .strip_prefix(scope.cwd())
        .ok()
        .and_then(|inside| inside.to_str())
        .unwrap_or(trimmed);
    project_path(scope, relative).await
}
