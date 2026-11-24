//! The project graph kept current. The core never reads a language here: it lists a
//! saved project's files, counts their lines and hashes them, and stores that beside the
//! facts agents wrote (`state::graph`). Only files whose size or change time moved are
//! read again; a fact whose file changed becomes stale and is kept; a file that is gone
//! takes its facts along.
//!
//! A refresh runs when a thread starts in a saved project, after each turn and after the
//! git operations the core serves, in the background (`refresh_soon`); the tool asks for
//! one inline when the last is older than it accepts (`refresh_within`). Refreshes of one
//! folder run one at a time, and each stops reading files at its time budget: the rest
//! wait for the next one, so a huge repository never holds a turn.

#[cfg(test)]
mod tests;
mod walk;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

pub use walk::{MAX_READ_BYTES, measure};

use crate::{
    project::episodes::saved_dir,
    state::{State, graph::WalkedFile},
};

/// How long a background refresh may read files.
const BACKGROUND_BUDGET: Duration = Duration::from_secs(10);

/// What a refresh did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Refreshed {
    /// Files the project holds.
    pub files: usize,
    /// Files read because they were new or changed.
    pub read: usize,
    /// Files dropped because they are gone.
    pub removed: usize,
    /// New or changed files left for the next refresh: the budget ran out.
    pub pending: usize,
}

/// When the last full refresh of a folder started; one lock per folder, so its refreshes
/// run one at a time.
type Last = Arc<tokio::sync::Mutex<Option<Instant>>>;

static FOLDERS: LazyLock<Mutex<HashMap<PathBuf, Last>>> = LazyLock::new(Mutex::default);

fn last_of(dir: &Path) -> Last {
    let mut folders = FOLDERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Arc::clone(folders.entry(dir.to_owned()).or_default())
}

/// Refreshes the graph of `dir` in the background when it is a saved project's folder.
/// Asked again while one runs, it runs once more after it, never twice at once.
pub fn refresh_soon(state: Arc<State>, dir: PathBuf, skip: Vec<PathBuf>) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let asked = Instant::now();
    handle.spawn(async move {
        let refreshed = refresh_since(&state, &dir, &skip, asked, None, BACKGROUND_BUDGET);
        if let Err(error) = refreshed.await {
            tracing::warn!("cannot refresh the project graph: {error:#}");
        }
    });
}

/// Refreshes the graph of `dir` unless a refresh that started within `age` finished;
/// `None` when the folder is not a saved project's. While a background refresh runs it
/// waits for it at most `budget`, then answers with what is stored.
pub async fn refresh_within(
    state: &State,
    dir: &Path,
    skip: &[PathBuf],
    age: Duration,
    budget: Duration,
) -> Result<Option<Refreshed>> {
    let asked = Instant::now().checked_sub(age).unwrap_or_else(Instant::now);
    refresh_since(state, dir, skip, asked, Some(budget), budget).await
}

async fn refresh_since(
    state: &State,
    dir: &Path,
    skip: &[PathBuf],
    asked: Instant,
    wait: Option<Duration>,
    budget: Duration,
) -> Result<Option<Refreshed>> {
    let Some(project) = saved_dir(state, dir).await? else {
        return Ok(None);
    };
    let last = last_of(dir);
    let mut last = match wait {
        None => last.lock().await,
        Some(wait) => match tokio::time::timeout(wait, last.lock()).await {
            Ok(last) => last,
            Err(_) => return Ok(Some(Refreshed::default())),
        },
    };
    if last.is_some_and(|started| started >= asked) {
        return Ok(Some(Refreshed::default()));
    }
    let started = Instant::now();
    let refreshed = refresh(state, &project, dir, skip, budget).await?;
    // A refresh that left files for later is not a full one.
    if refreshed.pending == 0 {
        *last = Some(started);
    }
    Ok(Some(refreshed))
}

/// Lists the files of `dir`, reads those whose stamp moved until `budget` runs out, and
/// stores the result for `project`. A folder that cannot be listed whole is left as it
/// was stored.
pub async fn refresh(
    state: &State,
    project: &str,
    dir: &Path,
    skip: &[PathBuf],
    budget: Duration,
) -> Result<Refreshed> {
    let Some(on_disk) = walk::list(dir, skip).await else {
        tracing::info!("the project is too large or slow to list for its graph");
        return Ok(Refreshed::default());
    };
    let stored = state.graph_stamps(project).await?;
    let gone: Vec<String> = stored
        .keys()
        .filter(|path| !on_disk.contains_key(*path))
        .cloned()
        .collect();
    let mut changed: Vec<String> = on_disk
        .iter()
        .filter(|(path, stamp)| stored.get(*path) != Some(stamp))
        .map(|(path, _)| path.clone())
        .collect();
    changed.sort_unstable();
    let root = dir.to_owned();
    let total = changed.len();
    let (measured, tried) =
        tokio::task::spawn_blocking(move || measure_all(&root, changed, budget))
            .await
            .context("the graph's reading of files stopped")?;
    state.graph_update(project, &measured, &gone).await?;
    Ok(Refreshed {
        files: on_disk.len(),
        read: measured.len(),
        removed: gone.len(),
        pending: total - tried,
    })
}

/// Measures `paths` in order until `budget` runs out, and says how many it tried. A
/// file that cannot be read now (gone, no permission) is skipped; a later refresh tries
/// it again, as its stamp is still not the stored one.
fn measure_all(root: &Path, paths: Vec<String>, budget: Duration) -> (Vec<WalkedFile>, usize) {
    let started = Instant::now();
    let mut measured = Vec::new();
    let mut tried = 0;
    for path in paths {
        if started.elapsed() >= budget {
            break;
        }
        tried += 1;
        match measure(root, &path) {
            Ok(file) => measured.push(file),
            Err(error) => tracing::debug!("cannot measure a file for the graph: {error}"),
        }
    }
    (measured, tried)
}
