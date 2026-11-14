//! A summary of the git repository the client runs in, for the line under the box:
//! the repository, its branch and commits, the changed files, and how far it is ahead
//! of or behind its upstream. Read with the git command; no git, no line.

use std::path::Path;

use tokio::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSummary {
    pub repository: String,
    /// `None` on a detached head.
    pub branch: Option<String>,
    pub commits: u64,
    pub changed: usize,
    /// Commits ahead of and behind the upstream, when there is one.
    pub ahead_behind: Option<(u64, u64)>,
}

/// `None` outside a repository, or where git is missing.
pub async fn summary(directory: &Path) -> Option<GitSummary> {
    let top = git(directory, &["rev-parse", "--show-toplevel"]).await?;
    let repository = Path::new(top.trim())
        .file_name()?
        .to_string_lossy()
        .into_owned();
    let status = git(directory, &["status", "--porcelain=v2", "--branch"]).await?;
    // A new repository has no commits yet, and rev-list then fails.
    let commits = git(directory, &["rev-list", "--count", "HEAD"])
        .await
        .and_then(|count| count.trim().parse().ok())
        .unwrap_or(0);
    let mut branch = None;
    let mut ahead_behind = None;
    let mut changed = 0;
    for line in status.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            branch = (head != "(detached)").then(|| head.to_owned());
        } else if let Some(counts) = line.strip_prefix("# branch.ab ") {
            let mut parts = counts.split_whitespace();
            let ahead = parts
                .next()
                .and_then(|part| part.trim_start_matches('+').parse().ok());
            let behind = parts
                .next()
                .and_then(|part| part.trim_start_matches('-').parse().ok());
            ahead_behind = ahead.zip(behind);
        } else if !line.starts_with('#') && !line.is_empty() {
            changed += 1;
        }
    }
    Some(GitSummary {
        repository,
        branch,
        commits,
        changed,
        ahead_behind,
    })
}

async fn git(directory: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
