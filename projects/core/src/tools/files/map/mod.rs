//! `project_graph`: what lies where in the project, by file and folder names only. What
//! a file holds is for `read_file` and `search_files`. The name is kept from the code
//! graph this replaced, so saved configs that switch the tool still apply.

#[cfg(test)]
mod tests;
mod text;

use std::{path::Path, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use ignore::overrides::{Override, OverrideBuilder};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::changes::project_path;
use crate::{
    BoxFuture,
    project::ledger,
    tools::{
        console::skipped_folders,
        schema::{self, parse_arguments},
        scope::ToolScope,
        tool::{Tool, ToolContext, ToolKind},
    },
};

/// Longest `path` taken, in characters.
const MAX_PATH_CHARS: usize = 1024;
/// Longest `find` taken, in characters.
const MAX_FIND_CHARS: usize = 256;
/// One walk may take this long; the ledger's cap on files holds too.
const WALK_BUDGET: Duration = Duration::from_secs(2);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    /// A folder of the project; the whole project when left out.
    path: Option<String>,
    /// Part of a path, or a glob such as `*.rs`.
    find: Option<String>,
}

pub struct ProjectMap;

impl Tool for ProjectMap {
    fn name(&self) -> &'static str {
        "project_graph"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Files
    }

    fn description(&self, _context: &ToolContext) -> String {
        "Map of this project's folders and files by name, without their content. A tree; \
         deep folders fold to `name/ (N files)`, give `path` to open one. `find` lists the \
         paths containing the text or matching a glob such as `*.rs`. Ignored files are \
         left out."
            .into()
    }

    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }

    fn available(&self, context: &ToolContext) -> bool {
        context.config.limits().project_graph
    }

    fn repeatable(&self, _arguments: &Value) -> bool {
        true
    }

    fn read_only(&self, _arguments: &Value) -> bool {
        true
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let Arguments { path, find } = parse_arguments(arguments)?;
            let matcher = match find.as_deref() {
                Some(find) => Some(Matcher::new(context.scope.cwd(), find)?),
                None => None,
            };
            let folder = match path.as_deref() {
                Some(path) => folder_of(&context.scope, path).await?,
                None => None,
            };
            let paths = walk(&context.scope, folder.as_deref()).await?;
            let (Some(matcher), Some(find)) = (matcher, find) else {
                return Ok(text::tree(folder.as_deref().unwrap_or("."), paths));
            };
            let found = paths
                .into_iter()
                .map(|path| match &folder {
                    Some(folder) => format!("{folder}/{path}"),
                    None => path,
                })
                .filter(|path| matcher.matches(path));
            Ok(text::found(found.collect(), find.trim()))
        })
    }
}

/// Every file under `folder` (the whole project for `None`), relative to it with `/`:
/// walked as the change ledger walks it, so ignore rules hold, `.git` and
/// the config folder are left out and no link is followed.
async fn walk(scope: &ToolScope, folder: Option<&str>) -> Result<Vec<String>> {
    let root = scope.cwd().to_owned();
    let dir = folder.map_or_else(|| root.clone(), |folder| root.join(folder));
    let is_dir = tokio::fs::metadata(&dir)
        .await
        .is_ok_and(|meta| meta.is_dir());
    if !is_dir {
        bail!("no folder `{}` in the project", folder.unwrap_or("."));
    }
    let skip = skipped_folders(scope);
    let snapshot = tokio::task::spawn_blocking(move || ledger::take_for(&dir, &skip, WALK_BUDGET))
        .await
        .context("the walk of the project stopped")?
        .context("too many files to map here; give `path` of a folder inside")?;
    Ok(snapshot.files.into_keys().collect())
}

/// `path` as a folder of the project, relative with `/`; `None` for the whole project.
/// An absolute path is taken when it is inside the project; then it passes the checks
/// of the other file tools: no `..`, no link that leads out.
async fn folder_of(scope: &ToolScope, path: &str) -> Result<Option<String>> {
    ensure!(
        path.chars().count() <= MAX_PATH_CHARS,
        "`path` is longer than {MAX_PATH_CHARS} characters"
    );
    let trimmed = path.trim();
    let relative = Path::new(trimmed)
        .strip_prefix(scope.cwd())
        .ok()
        .and_then(|inside| inside.to_str())
        .unwrap_or(trimmed);
    project_path(scope, relative).await
}

/// What `find` matches: a glob when it has a glob character, else a part of the path,
/// in any case.
enum Matcher {
    Part(String),
    Glob(Override),
}

impl Matcher {
    fn new(root: &Path, find: &str) -> Result<Self> {
        ensure!(
            find.chars().count() <= MAX_FIND_CHARS,
            "`find` is longer than {MAX_FIND_CHARS} characters"
        );
        let find = find.trim();
        ensure!(!find.is_empty(), "`find` is empty");
        if !find.contains(['*', '?', '[', '{']) {
            return Ok(Self::Part(find.to_lowercase()));
        }
        let mut glob = OverrideBuilder::new(root);
        glob.case_insensitive(true)
            .context("invalid glob")?
            .add(find)
            .context("invalid glob")?;
        Ok(Self::Glob(glob.build().context("invalid glob")?))
    }

    fn matches(&self, path: &str) -> bool {
        match self {
            Self::Part(part) => path.to_lowercase().contains(part.as_str()),
            Self::Glob(glob) => glob.matched(path, false).is_whitelist(),
        }
    }
}
