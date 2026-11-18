//! Fetch, pull and push. Git uses the user's own credential helper and keys; the core
//! never reads, stores or logs a token. A pull only fast-forwards and a push is never
//! forced: no argument that forces exists on any path here.

use anyhow::{Context, Result, bail};
use dusk_protocol::project::{GitOutcome, Project};

use super::{
    check,
    run::{LOCAL, NETWORK, checked, git, run},
    write::conflicts,
};

pub async fn fetch(project: &Project) -> Result<GitOutcome> {
    network(project, &["fetch", "--quiet"]).await
}

/// Fast-forward only: a pull that would merge or rebase is refused by git with its
/// reason.
pub async fn pull(project: &Project) -> Result<GitOutcome> {
    let mut outcome = network(project, &["pull", "--ff-only", "--no-rebase"]).await?;
    outcome.conflicts = conflicts(project).await?;
    Ok(outcome)
}

/// The current branch to its upstream, or to `origin` as its new upstream. A push the
/// remote refuses, such as to a protected default branch, fails with git's own message.
pub async fn push(project: &Project) -> Result<GitOutcome> {
    let branch = git(&project.dir, &["branch", "--show-current"]).await?;
    let branch = branch.trim();
    if branch.is_empty() {
        bail!("a detached head has no branch to push");
    }
    let remote = config(project, &format!("branch.{branch}.remote")).await?;
    let merge = config(project, &format!("branch.{branch}.merge")).await?;
    let upstream = remote.as_deref().zip(merge.as_deref());
    let args = push_args(branch, upstream)?;
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    network(project, &args).await
}

/// One config value; none when it is not set.
async fn config(project: &Project, key: &str) -> Result<Option<String>> {
    let output = run(
        &project.dir,
        &["config", "--get", "--end-of-options", key],
        LOCAL,
    )
    .await
    .context("git")?;
    let value = output.stdout.trim();
    Ok((output.success && !value.is_empty()).then(|| value.to_owned()))
}

/// The arguments of a push of `branch`, to its upstream `(remote, merge ref)` or to
/// `origin` as a new upstream. Never `--force`, `--force-with-lease`, `--mirror`,
/// `--delete` or a `+` refspec: every name passes `check::branch`, which refuses a
/// leading `-` or `+`, and one explicit refspec is built here.
pub(super) fn push_args(branch: &str, upstream: Option<(&str, &str)>) -> Result<Vec<String>> {
    let branch = check::branch(branch)?;
    let mut args = vec!["push".to_owned(), "--porcelain".to_owned()];
    let (remote, target) = match upstream {
        Some((remote, merge)) => {
            let target = merge
                .strip_prefix("refs/heads/")
                .context("the upstream is not a branch")?;
            (check::branch(remote)?, check::branch(target)?)
        }
        None => {
            args.push("--set-upstream".to_owned());
            ("origin", branch)
        }
    };
    args.extend([
        "--end-of-options".to_owned(),
        remote.to_owned(),
        format!("refs/heads/{branch}:refs/heads/{target}"),
    ]);
    Ok(args)
}

async fn network(project: &Project, args: &[&str]) -> Result<GitOutcome> {
    let output = checked(&project.dir, args, NETWORK).await?;
    Ok(GitOutcome {
        message: output.message(),
        conflicts: Vec::new(),
    })
}
