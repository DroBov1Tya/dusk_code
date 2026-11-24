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
        return inside_one(state, thread_id, home, now).await;
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

/// For a thread with no worktree of its own whose folder is inside another thread's,
/// such as an agent asked from a branch without `isolated`: `home`, with the project
/// lookups and git data of that worktree, so memory, variables and commits work as they
/// do for the thread that made it. Plain `home` otherwise.
async fn inside_one(
    state: &State,
    thread_id: &str,
    home: &Arc<ToolScope>,
    now: &Arc<ToolScope>,
) -> Arc<ToolScope> {
    let worktrees = match state.thread_worktrees().await {
        Ok(worktrees) => worktrees,
        Err(error) => {
            warn!("thread {thread_id}: cannot read the worktrees: {error:#}");
            return Arc::clone(home);
        }
    };
    let Some((worktree, below)) = worktrees.iter().find_map(|worktree| {
        let below = home.cwd().strip_prefix(&worktree.dir).ok()?;
        Some((worktree, below))
    }) else {
        return Arc::clone(home);
    };
    let project = Path::new(&worktree.project).join(below);
    if now.cwd() == home.cwd() && now.project_dir() == project {
        return Arc::clone(now);
    }
    let Some(project) = project.to_str() else {
        return Arc::clone(home);
    };
    let dir = home.cwd().to_string_lossy();
    match in_worktree(home, &dir, project, &worktree.branch).await {
        Ok(scope) => Arc::new(scope),
        Err(error) => {
            warn!(
                "thread {thread_id}: its folder is in the worktree {}, which is not usable: \
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
///
/// The refs are opened as the whole `agent/` family, not the thread's one branch: git
/// writes a ref through a new lock file beside it, which some sandboxes allow only with
/// the folder open. So a confined command could move or delete another thread's agent
/// branch, which the `branch` tool never does; the user's branches and the config stay
/// closed. A sandboxed thread in the main checkout can write all of `.git` anyway.
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
