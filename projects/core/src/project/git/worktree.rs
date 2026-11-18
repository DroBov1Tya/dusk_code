//! Worktrees: more checkouts of one repository. The core picks where a new one goes,
//! beside the repository, so a client never names a folder to write; a worktree with
//! work in it is never removed, and nothing is pruned.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::project::{GitOutcome, Project};

use super::{
    check,
    read::info,
    run::{LOCAL, checked, git},
};

/// A worktree for `branch` at `<repository>-worktrees/<branch>`, `/` read as `-`. The
/// branch is made from `base`, or the current commit, when git does not have it yet.
pub async fn add(project: &Project, branch: &str, base: Option<&str>) -> Result<GitOutcome> {
    let branch = check::branch(branch)?;
    let base = base.map(check::branch).transpose()?;
    let top = git(&project.dir, &["rev-parse", "--show-toplevel"]).await?;
    let path = place(Path::new(top.trim()), branch)?;
    ensure!(!path.exists(), "{} is already there", path.display());
    let path = path.to_str().context("the folder is not valid UTF-8")?;
    let exists = info(project).await?.branches.iter().any(|b| b == branch);
    let mut args = vec!["worktree", "add"];
    match exists {
        true => args.extend(["--end-of-options", path, branch]),
        false => {
            args.extend(["-b", branch, "--end-of-options", path]);
            args.extend(base);
        }
    }
    let output = checked(&project.dir, &args, LOCAL).await?;
    Ok(GitOutcome {
        message: output.message(),
        conflicts: Vec::new(),
    })
}

/// Removes a worktree git lists, never the main one, and only when it has no
/// uncommitted or untracked files: those would be lost with the folder.
pub async fn remove(project: &Project, path: &Path) -> Result<GitOutcome> {
    let listed = info(project).await?.worktrees;
    let main = listed
        .first()
        .context("the repository lists no worktrees")?;
    let real = tokio::fs::canonicalize(path)
        .await
        .context("no such worktree")?;
    let known = listed
        .iter()
        .skip(1)
        .any(|tree| std::fs::canonicalize(&tree.path).is_ok_and(|listed| listed == real));
    ensure!(
        std::fs::canonicalize(&main.path).ok().as_ref() != Some(&real),
        "the main worktree is the repository itself and is not removed"
    );
    ensure!(known, "not a worktree of this repository");
    let status = git(
        &real,
        &[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignored=no",
        ],
    )
    .await?;
    let dirty = status.lines().count();
    if dirty > 0 {
        bail!(
            "the worktree has {dirty} uncommitted or untracked file{}: commit or remove them first",
            if dirty == 1 { "" } else { "s" }
        );
    }
    let real = real.to_str().context("the folder is not valid UTF-8")?;
    let output = checked(
        &project.dir,
        &["worktree", "remove", "--end-of-options", real],
        LOCAL,
    )
    .await?;
    Ok(GitOutcome {
        message: output.message(),
        conflicts: Vec::new(),
    })
}

/// `<parent>/<name>-worktrees/<branch with / as ->`: beside the repository, never in it.
fn place(top: &Path, branch: &str) -> Result<PathBuf> {
    let name = top
        .file_name()
        .and_then(|name| name.to_str())
        .context("the repository folder has no name")?;
    let parent = top.parent().context("the repository is at the root")?;
    Ok(parent
        .join(format!("{name}-worktrees"))
        .join(branch.replace('/', "-")))
}
