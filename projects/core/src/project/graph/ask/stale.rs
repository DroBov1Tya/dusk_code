//! Which files of an answer changed on disk since they were indexed. Each is looked at
//! through the project's folder, never through a link that leads out of it.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use super::{Answer, MAX_CHECKED};
use crate::{
    project::{graph::refresh, ledger, plain_relative},
    state::{State, graph::Stamp},
};

/// The stamp of the indexed file at `path` as the disk has it now. `None` when it is no
/// longer a regular file of the project: gone, a link, or behind a folder that leads out.
pub fn on_disk(root: &Path, path: &str) -> Option<Stamp> {
    let relative = Path::new(path);
    if plain_relative(relative).is_err() {
        return None;
    }
    let absolute = root.join(relative);
    if !refresh::inside(root, &absolute) {
        return None;
    }
    ledger::Stamp::of(&absolute).map(refresh::stamp)
}

impl Answer {
    /// The indexed files the answer names, in the order shown, without repeats.
    pub(super) fn files(&self) -> Vec<String> {
        let paths: Vec<&str> = match self {
            Self::Definitions { rows, .. } => rows.iter().map(|row| row.path.as_str()).collect(),
            Self::Dependencies { rows, .. } => rows.iter().map(|row| row.path.as_str()).collect(),
            Self::Uses(uses) => uses
                .defined
                .iter()
                .map(|defined| defined.path.as_str())
                .chain(uses.rows.iter().map(|row| row.path.as_str()))
                .collect(),
            Self::Dependents(found) => std::iter::once(found.path.as_str())
                .chain(found.code.iter().map(|row| row.path.as_str()))
                .chain(found.tests.iter().map(String::as_str))
                .collect(),
            Self::Tests {
                subject,
                file,
                rows,
            } => file
                .then_some(subject.as_str())
                .into_iter()
                .chain(rows.iter().map(|row| row.path.as_str()))
                .collect(),
            Self::File(file) => vec![file.path.as_str()],
            Self::NotParsed { path, .. } => vec![path.as_str()],
            Self::TooCommon { .. } | Self::Miss(_) => Vec::new(),
        };
        let mut seen = HashSet::new();
        paths
            .into_iter()
            .filter(|path| seen.insert(*path))
            .map(str::to_owned)
            .collect()
    }
}

/// The files whose stamp on disk is not the indexed one, of the first `MAX_CHECKED`.
pub(super) async fn changed_since_indexed(
    state: &State,
    project: &str,
    root: &Path,
    paths: Vec<String>,
) -> Result<HashSet<String>> {
    let mut stored = Vec::new();
    for path in paths.into_iter().take(MAX_CHECKED) {
        if let Some(file) = state.graph_file(project, &path).await? {
            stored.push((path, file.stamp));
        }
    }
    let root: PathBuf = root.to_owned();
    tokio::task::spawn_blocking(move || {
        stored
            .into_iter()
            .filter(|(path, stamp)| on_disk(&root, path) != Some(*stamp))
            .map(|(path, _)| path)
            .collect()
    })
    .await
    .context("compare the files with the disk")
}
