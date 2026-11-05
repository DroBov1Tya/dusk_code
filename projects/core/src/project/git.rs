//! A project's repository through the git command: fixed arguments, no shell, no
//! prompts, and a time limit. A branch is switched to only when git already has it.

use std::{path::Path, process::Stdio, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::project::{Change, ChangeKind, GitInfo, Project, Worktree};
use tokio::process::Command;

const TIMEOUT: Duration = Duration::from_secs(10);

/// An empty record outside a repository, or where git is missing.
pub async fn info(project: &Project) -> Result<GitInfo> {
    let dir = &project.dir;
    let Ok(prefix) = git(dir, &["rev-parse", "--show-prefix"]).await else {
        return Ok(GitInfo::default());
    };
    let branch = git(dir, &["branch", "--show-current"]).await?;
    let branches = git(
        dir,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .await?;
    let worktrees = git(dir, &["worktree", "list", "--porcelain"]).await?;
    let status = git(
        dir,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    .await?;
    // No upstream is no error worth showing: the counts are just left out.
    let ahead_behind = git(dir, &["rev-list", "--left-right", "--count", "HEAD...@{u}"])
        .await
        .ok()
        .and_then(|counts| {
            let mut parts = counts.split_whitespace().map(str::parse::<u64>);
            Some((parts.next()?.ok()?, parts.next()?.ok()?))
        });
    let branch = branch.trim();
    Ok(GitInfo {
        branch: (!branch.is_empty()).then(|| branch.to_owned()),
        branches: branches.lines().map(str::to_owned).collect(),
        worktrees: parse_worktrees(&worktrees),
        ahead_behind,
        changes: parse_status(&status, prefix.trim()),
    })
}

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
    // The name is one git listed, so it cannot be read as an option.
    git(&project.dir, &["switch", "--no-guess", branch]).await?;
    Ok(())
}

async fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(TIMEOUT, command.output())
        .await
        .context("git took too long")?
        .context("cannot run git")?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        bail!("git: {}", error.trim());
    }
    String::from_utf8(output.stdout).context("git printed text that is not UTF-8")
}

/// Paths come relative to the repository; `prefix` is where the project sits inside it,
/// and changes outside the project are left out.
fn parse_status(status: &str, prefix: &str) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut entries = status.split('\0').filter(|entry| !entry.is_empty());
    while let Some(entry) = entries.next() {
        let (Some(code), Some(path)) = (entry.get(..2), entry.get(3..)) else {
            continue;
        };
        if code.contains('R') || code.contains('C') {
            // A rename carries the old path next; only the new one is shown.
            entries.next();
        }
        let Some(path) = path.strip_prefix(prefix) else {
            continue;
        };
        changes.push(Change {
            path: path.trim_end_matches('/').to_owned(),
            kind: kind(code),
        });
    }
    changes
}

fn kind(code: &str) -> ChangeKind {
    match code {
        "??" => ChangeKind::Untracked,
        "AA" | "DD" => ChangeKind::Conflicted,
        _ if code.contains('U') => ChangeKind::Conflicted,
        _ if code.contains('R') || code.contains('C') => ChangeKind::Renamed,
        _ if code.contains('A') => ChangeKind::Added,
        _ if code.contains('D') => ChangeKind::Deleted,
        _ => ChangeKind::Modified,
    }
}

fn parse_worktrees(text: &str) -> Vec<Worktree> {
    let mut worktrees: Vec<Worktree> = Vec::new();
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            worktrees.push(Worktree {
                path: path.into(),
                branch: None,
            });
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/")
            && let Some(last) = worktrees.last_mut()
        {
            last.branch = Some(branch.to_owned());
        }
    }
    worktrees
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_read_inside_the_project() {
        let status =
            " M app/src/a.rs\0R  app/b.rs\0app/old.rs\0?? app/new/\0 M other/c.rs\0UU app/d.rs\0";
        let changes = parse_status(status, "app/");
        let shown: Vec<_> = changes
            .iter()
            .map(|change| (change.path.as_str(), change.kind))
            .collect();
        assert_eq!(
            shown,
            [
                ("src/a.rs", ChangeKind::Modified),
                ("b.rs", ChangeKind::Renamed),
                ("new", ChangeKind::Untracked),
                ("d.rs", ChangeKind::Conflicted),
            ]
        );
    }

    #[test]
    fn worktrees_carry_their_branch() {
        let text = "worktree /a\nHEAD 1\nbranch refs/heads/main\n\nworktree /b\nHEAD 2\ndetached\n";
        let worktrees = parse_worktrees(text);
        assert_eq!(worktrees[0].branch.as_deref(), Some("main"));
        assert_eq!(worktrees[1].branch, None);
    }
}
