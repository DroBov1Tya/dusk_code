//! Where a thread's tools act once it branched off: its worktree, as the record says,
//! so a restart picks it up again; its own folder when it has none.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use tracing::warn;

use crate::{
    project::git::{self, Repository},
    state::State,
    tools::ToolScope,
};

/// The scope for `thread_id`: `now` while it still fits, `home` when the thread has no
/// worktree or it is gone, else a scope over the worktree.
pub async fn scope_for(
    state: &State,
    thread_id: &str,
    home: &Arc<ToolScope>,
    now: &Arc<ToolScope>,
) -> Arc<ToolScope> {
    let worktree = match state.thread_worktree(thread_id).await {
        Ok(worktree) => worktree,
        Err(error) => {
            warn!("thread {thread_id}: cannot read its worktree: {error:#}");
            return Arc::clone(now);
        }
    };
    let Some(worktree) = worktree else {
        return Arc::clone(home);
    };
    if now.cwd() == Path::new(&worktree.dir) {
        return Arc::clone(now);
    }
    match in_worktree(home, &worktree.dir, &worktree.project, &worktree.branch).await {
        Ok(scope) => Arc::new(scope),
        Err(error) => {
            warn!(
                "thread {thread_id}: works in its own folder, its worktree {} is not usable: \
                 {error:#}",
                worktree.dir
            );
            Arc::clone(home)
        }
    }
}

/// `home` moved to the worktree at `dir` on `branch`, for the project at `project`.
pub async fn in_worktree(
    home: &ToolScope,
    dir: &str,
    project: &str,
    branch: &str,
) -> Result<ToolScope> {
    let repository = git::repository(Path::new(dir))
        .await
        .context("the worktree is no repository")?;
    home.in_worktree(
        Path::new(dir),
        Path::new(project),
        git_writable(&repository, branch),
    )
}

/// What a confined `git commit` in a worktree writes outside it: the shared objects, the
/// refs and reflogs of the agents' branches, and the worktree's own data. Never the
/// shared config or hooks, which run as code in every checkout. Only folders that are
/// there, since a missing one cannot be opened to a confinement.
pub fn git_writable(repository: &Repository, branch: &str) -> Vec<PathBuf> {
    let common = &repository.common;
    let mut folders = vec![common.join("objects")];
    if let Some((family, _)) = branch.rsplit_once('/') {
        folders.push(common.join("refs/heads").join(family));
        folders.push(common.join("logs/refs/heads").join(family));
    }
    // A linked worktree's own data is under the shared data; the main checkout's is
    // the shared data itself, which is never opened whole.
    if repository.own != *common && repository.own.starts_with(common.join("worktrees")) {
        folders.push(repository.own.clone());
    }
    folders.retain(|folder| folder.is_dir());
    folders
}
