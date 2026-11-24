//! What a branch an agent works on needs of git beyond the rest of this module: where a
//! folder's repository keeps its data, how far two branches are apart, and removing a
//! branch only once nothing on it would be lost. Names pass `check` first.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::{
    check,
    run::{LOCAL, git, run},
};

/// A folder's place in its repository.
#[derive(Debug, Clone)]
pub struct Repository {
    /// The top of the checkout the folder is in: the repository or one of its worktrees.
    pub top: PathBuf,
    /// Where the folder sits under `top`, `/`-ended; empty at the top.
    pub prefix: String,
    /// `None` on a detached head.
    pub branch: Option<String>,
    /// The data every checkout shares: objects, refs, config and hooks.
    pub common: PathBuf,
    /// The checkout's own data: its HEAD, index and merge state.
    pub own: PathBuf,
}

/// `None` outside a repository or where git is missing.
pub async fn repository(dir: &Path) -> Option<Repository> {
    let args = [
        "rev-parse",
        "--path-format=absolute",
        "--show-toplevel",
        "--git-common-dir",
        "--git-dir",
        "--show-prefix",
    ];
    let text = git(dir, &args).await.ok()?;
    let mut lines = text.lines();
    let top = PathBuf::from(lines.next()?);
    let common = PathBuf::from(lines.next()?);
    let own = PathBuf::from(lines.next()?);
    let prefix = lines.next().unwrap_or_default().to_owned();
    let branch = git(dir, &["branch", "--show-current"]).await.ok()?;
    let branch = branch.trim();
    Some(Repository {
        top: top.canonicalize().ok()?,
        prefix,
        branch: (!branch.is_empty()).then(|| branch.to_owned()),
        common: common.canonicalize().ok()?,
        own: own.canonicalize().ok()?,
    })
}

/// Commits on `branch` that `base` lacks, and commits on `base` that `branch` lacks.
pub async fn apart(dir: &Path, base: &str, branch: &str) -> Result<(u64, u64)> {
    let range = format!("{}...{}", check::branch(base)?, check::branch(branch)?);
    let counts = git(
        dir,
        &[
            "rev-list",
            "--left-right",
            "--count",
            "--end-of-options",
            &range,
        ],
    )
    .await?;
    let mut parts = counts.split_whitespace().map(str::parse::<u64>);
    let behind = parts.next().context("git gave no count")??;
    let ahead = parts.next().context("git gave no count")??;
    Ok((ahead, behind))
}

/// Every uncommitted file of the checkout `dir` is in, wherever in it; untracked ones
/// too with `untracked`.
pub async fn unsaved(dir: &Path, untracked: bool) -> Result<Vec<String>> {
    let untracked = match untracked {
        true => "--untracked-files=all",
        false => "--untracked-files=no",
    };
    let status = git(
        dir,
        &["status", "--porcelain=v1", untracked, "--ignored=no"],
    )
    .await?;
    Ok(status
        .lines()
        .filter_map(|line| line.get(3..))
        .map(str::to_owned)
        .collect())
}

/// The files `branch` changed since it left `base`, relative to the repository.
pub async fn files_between(dir: &Path, base: &str, branch: &str) -> Result<Vec<String>> {
    let range = format!("{}...{}", check::branch(base)?, check::branch(branch)?);
    let names = git(
        dir,
        &[
            "diff",
            "--name-only",
            "--no-renames",
            "--end-of-options",
            &range,
        ],
    )
    .await?;
    Ok(names.lines().map(str::to_owned).collect())
}

/// Whether a merge waits in the checkout `dir` is in, to be finished or aborted.
pub async fn merging(dir: &Path) -> bool {
    run(dir, &["rev-parse", "-q", "--verify", "MERGE_HEAD"], LOCAL)
        .await
        .is_ok_and(|output| output.success)
}

/// Removes `branch` when its work is merged into the branch checked out in `dir`; git
/// refuses otherwise (`branch -d`, never `-D`), and then it stays. Whether it went.
pub async fn remove_merged(dir: &Path, branch: &str) -> Result<bool> {
    let branch = check::branch(branch)?;
    let output = run(
        dir,
        &["branch", "--delete", "--end-of-options", branch],
        LOCAL,
    )
    .await?;
    Ok(output.success)
}

/// The branch names a new one may not take: those git has.
pub async fn names(dir: &Path) -> Result<Vec<String>> {
    let names = git(
        dir,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .await?;
    Ok(names.lines().map(str::to_owned).collect())
}
