//! Local changes of a repository: branches, the index, commits, discarding a file and
//! merging. Names and paths pass `check` first; paths go after `--` and are read
//! literally, never as patterns.

use anyhow::{Result, ensure};
use dusk_protocol::project::{ChangeKind, GitOutcome, Project};

use super::{
    check,
    read::info,
    run::{LOCAL, checked, git, run},
};

pub async fn create_branch(
    project: &Project,
    name: &str,
    base: Option<&str>,
    switch: bool,
) -> Result<GitOutcome> {
    let name = check::branch(name)?;
    let base = base.map(check::branch).transpose()?;
    // `git switch` carries the work tree's changes along and refuses to lose any.
    let create = format!("--create={name}");
    let mut args = if switch {
        vec!["switch", "--no-track", &create, "--end-of-options"]
    } else {
        vec!["branch", "--no-track", "--end-of-options", name]
    };
    args.extend(base);
    done(project, &args).await
}

/// Refused while tracked files have changes, so nothing is lost.
pub async fn switch_branch(project: &Project, branch: &str) -> Result<()> {
    let info = info(project).await?;
    ensure!(
        info.branches.iter().any(|known| known == branch),
        "no such branch"
    );
    let tracked = info
        .changes
        .iter()
        .any(|change| change.kind != ChangeKind::Untracked);
    ensure!(!tracked, "commit or stash the changes before switching");
    // The name is one git listed, and `--end-of-options` keeps it a name.
    git(
        &project.dir,
        &["switch", "--no-guess", "--end-of-options", branch],
    )
    .await?;
    Ok(())
}

pub async fn stage(project: &Project, paths: &[String]) -> Result<GitOutcome> {
    with_paths(project, &["add", "--all"], paths).await
}

/// Every change inside the project folder, new files included.
pub async fn stage_all(project: &Project) -> Result<GitOutcome> {
    done(project, &["add", "--all", "--", "."]).await
}

pub async fn unstage(project: &Project, paths: &[String]) -> Result<GitOutcome> {
    with_paths(project, &["restore", "--staged"], paths).await
}

/// Commits what is staged. There is no amend: a commit only ever adds one.
pub async fn commit(project: &Project, message: &str) -> Result<GitOutcome> {
    let message = format!("--message={}", check::message(message)?);
    done(project, &["commit", "--quiet", &message]).await
}

/// Throws away the file's uncommitted changes, staged ones too; an untracked file is
/// removed. The one destructive local request: a client asks before sending it.
pub async fn discard(project: &Project, path: &str) -> Result<GitOutcome> {
    let path = check::path(project, path).await?;
    let info = info(project).await?;
    let change = info
        .changes
        .iter()
        .find(|change| change.path == path)
        .ok_or_else(|| anyhow::anyhow!("{path} has no changes"))?;
    let args: &[&str] = match change.kind {
        ChangeKind::Untracked => &["clean", "--force", "--quiet"],
        _ => &["restore", "--source=HEAD", "--staged", "--worktree"],
    };
    with_paths(project, args, &[path.to_owned()]).await
}

/// Merges `branch` into the current branch. Conflicts are an answer, not an error: the
/// files are listed and the merge waits to be finished or aborted.
pub async fn merge(project: &Project, branch: &str) -> Result<GitOutcome> {
    merge_with(project, branch, &[]).await
}

/// `merge`, always as a merge commit (`--no-ff`), so the branch's work stays one unit
/// in the history that can be found and reverted.
pub async fn merge_recorded(project: &Project, branch: &str) -> Result<GitOutcome> {
    merge_with(project, branch, &["--no-ff"]).await
}

async fn merge_with(project: &Project, branch: &str, options: &[&str]) -> Result<GitOutcome> {
    let branch = check::branch(branch)?;
    let mut args = vec!["merge", "--no-edit"];
    args.extend_from_slice(options);
    args.extend(["--end-of-options", branch]);
    let output = run(&project.dir, &args, LOCAL).await?;
    let conflicts = conflicts(project).await?;
    ensure!(
        output.success || !conflicts.is_empty(),
        "git: {}",
        output.message()
    );
    Ok(GitOutcome {
        message: output.message(),
        conflicts,
    })
}

pub async fn abort_merge(project: &Project) -> Result<GitOutcome> {
    done(project, &["merge", "--abort"]).await
}

/// The files left in conflict, relative to the project folder.
pub(super) async fn conflicts(project: &Project) -> Result<Vec<String>> {
    Ok(info(project)
        .await?
        .changes
        .into_iter()
        .filter(|change| change.kind == ChangeKind::Conflicted)
        .map(|change| change.path)
        .collect())
}

/// `args`, then `--` and the checked paths, read literally.
async fn with_paths(project: &Project, args: &[&str], paths: &[String]) -> Result<GitOutcome> {
    ensure!(!paths.is_empty(), "no paths given");
    let mut checked_paths = Vec::with_capacity(paths.len());
    for path in paths {
        checked_paths.push(check::path(project, path).await?);
    }
    let mut all = vec!["--literal-pathspecs"];
    all.extend_from_slice(args);
    all.push("--");
    all.extend(checked_paths);
    done(project, &all).await
}

async fn done(project: &Project, args: &[&str]) -> Result<GitOutcome> {
    let output = checked(&project.dir, args, LOCAL).await?;
    Ok(GitOutcome {
        message: output.message(),
        conflicts: Vec::new(),
    })
}
