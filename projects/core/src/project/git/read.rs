//! Reading a project's repository: its branch, changes, branches and worktrees.

use std::path::Path;

use anyhow::Result;
use dusk_protocol::project::{Change, ChangeKind, GitInfo, GitSummary, Project, Worktree};

use super::run::git;

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
    let ahead_behind = ahead_behind(dir).await;
    let web = git(dir, &["remote", "get-url", "origin"])
        .await
        .ok()
        .and_then(|remote| web_page(remote.trim()));
    let branch = branch.trim();
    let changes = parse_status(&status, prefix.trim());
    let stat = super::super::diff::stat(dir, &changes).await;
    Ok(GitInfo {
        branch: (!branch.is_empty()).then(|| branch.to_owned()),
        branches: branches.lines().map(str::to_owned).collect(),
        worktrees: parse_worktrees(&worktrees),
        ahead_behind,
        changes,
        web,
        stat,
    })
}

/// The repository the client's own folder is in, for its status line. Unlike `info`
/// this is not bound to a saved project: it reads only names and counts, never files.
/// `None` outside a repository or where git is missing.
pub async fn summary(dir: &Path) -> Option<GitSummary> {
    let top = git(dir, &["rev-parse", "--show-toplevel"]).await.ok()?;
    let repository = Path::new(top.trim())
        .file_name()?
        .to_string_lossy()
        .into_owned();
    let branch = git(dir, &["branch", "--show-current"]).await.ok()?;
    // Untracked folders count once, as `git status` lists them.
    let status = git(dir, &["status", "--porcelain=v1", "-z"]).await.ok()?;
    // A new repository has no commits yet, and rev-list then fails.
    let commits = git(dir, &["rev-list", "--count", "HEAD"])
        .await
        .ok()
        .and_then(|count| count.trim().parse().ok())
        .unwrap_or(0);
    let branch = branch.trim();
    Some(GitSummary {
        repository,
        branch: (!branch.is_empty()).then(|| branch.to_owned()),
        commits,
        changed: parse_status(&status, "").len(),
        ahead_behind: ahead_behind(dir).await,
    })
}

/// Commits ahead of and behind the upstream. No upstream is no error worth showing:
/// the counts are just left out.
async fn ahead_behind(dir: &Path) -> Option<(u64, u64)> {
    let counts = git(dir, &["rev-list", "--left-right", "--count", "HEAD...@{u}"])
        .await
        .ok()?;
    let mut parts = counts.split_whitespace().map(str::parse::<u64>);
    Some((parts.next()?.ok()?, parts.next()?.ok()?))
}

/// The https page of a remote such as `git@github.com:owner/repo.git`,
/// `ssh://git@host/owner/repo` or `https://user:key@host/owner/repo.git`. Credentials
/// are dropped; a host or path with anything but plain characters gives none.
pub(in crate::project) fn web_page(remote: &str) -> Option<String> {
    let rest = remote
        .strip_prefix("https://")
        .or_else(|| remote.strip_prefix("http://"))
        .or_else(|| remote.strip_prefix("ssh://"));
    let (host, path) = match rest {
        Some(rest) => {
            // Userinfo belongs to the authority only: an `@` in the path never
            // changes the host.
            let (authority, path) = rest.split_once('/')?;
            let host = authority
                .rsplit_once('@')
                .map_or(authority, |(_, after)| after);
            // An ssh port is not the web server's.
            (host.split(':').next()?, path)
        }
        None => {
            let (user_host, path) = remote.split_once(':')?;
            (user_host.rsplit_once('@')?.1, path)
        }
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let host_ok = !host.is_empty()
        && host.contains('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    let path_ok = !path.is_empty()
        && !path.split('/').any(|part| part.is_empty() || part == "..")
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-~/".contains(c));
    (host_ok && path_ok).then(|| format!("https://{host}/{path}"))
}

/// The files git reports as changed in `dir`, or none outside a repository.
pub async fn changes(dir: &Path) -> Vec<Change> {
    let Ok(prefix) = git(dir, &["rev-parse", "--show-prefix"]).await else {
        return Vec::new();
    };
    let args = ["status", "--porcelain=v1", "-z", "--untracked-files=all"];
    match git(dir, &args).await {
        Ok(status) => parse_status(&status, prefix.trim()),
        Err(_) => Vec::new(),
    }
}

/// Paths come relative to the repository; `prefix` is where the project sits inside it,
/// and changes outside the project are left out.
pub(super) fn parse_status(status: &str, prefix: &str) -> Vec<Change> {
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
            staged: !matches!(code.as_bytes()[0], b' ' | b'?' | b'!'),
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
    fn a_remote_gives_its_page_without_credentials() {
        for (remote, page) in [
            (
                "git@github.com:owner/repo.git",
                Some("https://github.com/owner/repo"),
            ),
            (
                "ssh://git@gitlab.com:2222/group/sub/repo.git",
                Some("https://gitlab.com/group/sub/repo"),
            ),
            (
                "https://user:secret@github.com/owner/repo.git",
                Some("https://github.com/owner/repo"),
            ),
            (
                "https://github.com/owner/repo/",
                Some("https://github.com/owner/repo"),
            ),
            ("/srv/git/repo.git", None),
            ("file:///etc/passwd", None),
            ("https://github.com/owner/../../x", None),
            ("https://evil.com/a?b=<script>", None),
            ("git@local:repo", None),
        ] {
            assert_eq!(web_page(remote).as_deref(), page, "{remote}");
        }
    }

    #[test]
    fn status_is_read_inside_the_project() {
        let status =
            " M app/src/a.rs\0R  app/b.rs\0app/old.rs\0?? app/new/\0 M other/c.rs\0UU app/d.rs\0";
        let changes = parse_status(status, "app/");
        let shown: Vec<_> = changes
            .iter()
            .map(|change| (change.path.as_str(), change.kind, change.staged))
            .collect();
        assert_eq!(
            shown,
            [
                ("src/a.rs", ChangeKind::Modified, false),
                ("b.rs", ChangeKind::Renamed, true),
                ("new", ChangeKind::Untracked, false),
                ("d.rs", ChangeKind::Conflicted, true),
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
