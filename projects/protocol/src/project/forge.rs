//! A project's pull requests, as the GitHub CLI reports them.

use serde::{Deserialize, Serialize};

/// How a pull request's checks, or one check, stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckState {
    Pending,
    Passing,
    Failing,
}

/// Text fields are plain and capped by the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub branch: String,
    pub base: String,
    pub added: u64,
    pub removed: u64,
    pub draft: bool,
    /// All checks together; `None` when there are none.
    pub checks: Option<CheckState>,
    /// The page on GitHub, https only.
    pub url: String,
}

/// Where one check stands, finer than `CheckState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckStage {
    Queued,
    Running,
    Passed,
    Skipped,
    Failed,
}

impl CheckStage {
    pub fn state(self) -> CheckState {
        match self {
            CheckStage::Queued | CheckStage::Running => CheckState::Pending,
            CheckStage::Passed | CheckStage::Skipped => CheckState::Passing,
            CheckStage::Failed => CheckState::Failing,
        }
    }
}

/// One check of a pull request; text plain and capped by the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    /// The workflow it runs in; empty when GitHub names none.
    pub workflow: String,
    pub stage: CheckStage,
    /// Its page, https only; empty when GitHub gave none that is safe.
    pub url: String,
    /// Unix seconds; `None` when not known.
    pub started: Option<i64>,
    pub finished: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestDetail {
    pub pull: PullRequest,
    /// `OPEN`, `CLOSED` or `MERGED`.
    pub state: String,
    pub body: String,
    pub checks: Vec<Check>,
}

/// Where a pull request stands on GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PullState {
    Open,
    Draft,
    Merged,
    Closed,
}

/// A pull request's checks counted. `state` is the worst of them: failing over
/// running over passing; `None` when there are none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checks {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub running: u32,
    pub state: Option<CheckState>,
    /// The checks themselves: failed first, then running, queued, passed and skipped;
    /// capped, so `total` may count more.
    pub list: Vec<Check>,
}

/// One pull request as the repository status shows it; text plain and capped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullStatus {
    pub number: u64,
    pub title: String,
    pub state: PullState,
    /// The head branch.
    pub branch: String,
    /// The author's login; empty when GitHub gave none.
    pub author: String,
    /// Unix seconds; `None` when not known.
    pub created: Option<i64>,
    pub updated: Option<i64>,
    pub added: u64,
    pub removed: u64,
    /// How many files it changes; `None` when not known.
    pub files: Option<u64>,
    /// The page on GitHub, https only; empty when GitHub gave none that is safe.
    pub url: String,
    pub checks: Checks,
}

/// A project's repository at a glance: the work tree's changes and the pull requests
/// that matter to it. The core keeps it fresh while a client shows it and sends
/// `Event::RepositoryStatus` when it changes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryStatus {
    /// The name of the repository's top folder; empty outside a repository.
    pub repository: String,
    /// `owner/repo` of `origin` when it is on github.com; `None` otherwise.
    pub github: Option<String>,
    /// `None` on a detached head or outside a repository.
    pub branch: Option<String>,
    /// Lines added and removed in the work tree against the last commit.
    pub stat: crate::project::DiffStat,
    /// The current branch's pull requests first, then the other open ones; capped.
    pub pulls: Vec<PullStatus>,
    /// Why there are no pull requests to show: GitHub CLI missing or signed out, not a
    /// GitHub remote, or GitHub not answering. Plain, for a quiet note; `None` when read.
    pub unavailable: Option<String>,
}

/// The pull requests of a project's current branch, for an icon beside its chats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullSummary {
    pub project_id: String,
    pub pulls: Vec<PullStatus>,
    /// The worst checks state of them all; `None` when none has checks.
    pub checks: Option<CheckState>,
}

/// The number of the pull request `url` points to when it starts with `prefix`, the
/// pull link base the forge plugin gave (ends with `/`); `None` for any other link.
pub fn pull_number(url: &str, prefix: &str) -> Option<u64> {
    if !prefix.ends_with('/') {
        return None;
    }
    let head = url.as_bytes().get(..prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix.as_bytes()) {
        return None;
    }
    let rest = url.get(prefix.len()..)?;
    let end = rest.find(['/', '#', '?']).unwrap_or(rest.len());
    let digits = &rest[..end];
    (digits.len() <= 12 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse::<u64>().ok())
        .flatten()
        .filter(|number| *number > 0)
}

#[cfg(test)]
mod tests {
    use super::pull_number;

    #[test]
    fn only_a_link_under_the_prefix_has_a_number() {
        let prefix = "https://github.com/DroBov1Tya/dusk_code/pull/";
        let base = "https://github.com/DroBov1Tya/dusk_code/pull";
        assert_eq!(pull_number(&format!("{base}/13"), prefix), Some(13));
        assert_eq!(pull_number(&format!("{base}/13/files"), prefix), Some(13));
        assert_eq!(pull_number(&format!("{base}/13#top"), prefix), Some(13));
        assert_eq!(pull_number(&format!("{base}/13?x=1"), prefix), Some(13));
        assert_eq!(
            pull_number("https://github.com/drobov1tya/DUSK_CODE/pull/7", prefix),
            Some(7)
        );
        for other in [
            "https://github.com/someone/else/pull/13".to_owned(),
            "https://github.com/DroBov1Tya/dusk_code_fork/pull/13".to_owned(),
            "https://github.com/DroBov1Tya/dusk/pull/13".to_owned(),
            "http://github.com/DroBov1Tya/dusk_code/pull/13".to_owned(),
            "https://github.com.evil.com/DroBov1Tya/dusk_code/pull/13".to_owned(),
            "https://gitlab.com/DroBov1Tya/dusk_code/pull/13".to_owned(),
            format!("{base}/"),
            format!("{base}/0"),
            format!("{base}/12a"),
            format!("{base}/99999999999999999999"),
            "https://github.com/DroBov1Tya/dusk_code/issues/13".to_owned(),
            "é".to_owned(),
        ] {
            assert_eq!(pull_number(&other, prefix), None, "{other}");
        }
        assert_eq!(pull_number(&format!("{base}/13"), ""), None);
        assert_eq!(pull_number(&format!("{base}/13"), base), None);
    }
}
