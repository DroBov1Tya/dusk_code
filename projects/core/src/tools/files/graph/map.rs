//! `map`: what lies where. In a saved project it is the stored graph, with lines and
//! summaries; elsewhere, or while nothing is stored yet, a walk by names alone.

use std::{path::Path, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use ignore::overrides::{Override, OverrideBuilder};

use super::{
    Arguments, Graph,
    text::{self, Entry},
};
use crate::{
    project::ledger,
    tools::{
        console::skipped_folders, files::changes::project_path, scope::ToolScope, tool::ToolContext,
    },
};

/// Longest `path` taken, in characters.
pub(super) const MAX_PATH_CHARS: usize = 1024;
/// Longest `find` taken, in characters.
const MAX_FIND_CHARS: usize = 256;
/// One walk may take this long; the ledger's cap on files holds too.
const WALK_BUDGET: Duration = Duration::from_secs(2);

pub(super) async fn answer(
    context: &ToolContext,
    graph: Option<&Graph<'_>>,
    arguments: &Arguments,
) -> Result<String> {
    let scope = &context.scope;
    let matcher = match arguments.find.as_deref() {
        Some(find) => Some(Matcher::new(scope.cwd(), find)?),
        None => None,
    };
    let folder = match arguments.path.as_deref() {
        Some(path) => folder_of(scope, path).await?,
        None => None,
    };
    let stored = match graph {
        Some(graph) => {
            graph
                .state
                .graph_map(&graph.project, folder.as_deref())
                .await?
        }
        None => Vec::new(),
    };
    let entries: Vec<Entry> = if stored.is_empty() {
        walk(scope, folder.as_deref())
            .await?
            .into_iter()
            .map(|path| Entry::named(full(folder.as_deref(), path)))
            .collect()
    } else {
        stored.into_iter().map(Entry::from).collect()
    };
    let mut answer = match (matcher, arguments.find.as_deref()) {
        (Some(matcher), Some(find)) => {
            let found = entries
                .into_iter()
                .filter(|entry| matcher.matches(&entry.path))
                .collect();
            text::found(found, find.trim())
        }
        _ => text::tree(folder.as_deref(), entries),
    };
    if let Some(graph) = graph
        && graph.pending > 0
    {
        answer.push_str(&format!(
            "[{} new or changed files not counted yet]\n",
            graph.pending
        ));
    }
    Ok(answer)
}

fn full(folder: Option<&str>, path: String) -> String {
    match folder {
        Some(folder) => format!("{folder}/{path}"),
        None => path,
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
    let folder = inside(scope, path).await?;
    if let Some(folder) = &folder {
        let is_dir = tokio::fs::metadata(scope.cwd().join(folder))
            .await
            .is_ok_and(|meta| meta.is_dir());
        ensure!(is_dir, "no folder `{folder}` in the project");
    }
    Ok(folder)
}

/// `path` relative to the project with `/`, checked as the other file tools check it;
/// `None` for the project itself.
pub(super) async fn inside(scope: &ToolScope, path: &str) -> Result<Option<String>> {
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

/// Checks `find` as `map` does, for the `find` action.
pub(super) fn check_find(find: Option<&str>) -> Result<&str> {
    let find = find.context("give `find`: the text to look for")?;
    ensure!(
        find.chars().count() <= MAX_FIND_CHARS,
        "`find` is longer than {MAX_FIND_CHARS} characters"
    );
    let find = find.trim();
    ensure!(!find.is_empty(), "`find` is empty");
    Ok(find)
}
