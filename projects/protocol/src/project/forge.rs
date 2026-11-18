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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub state: CheckState,
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checks {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub running: u32,
    pub state: Option<CheckState>,
}

/// One pull request as the repository status shows it; text plain and capped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullStatus {
    pub number: u64,
    pub title: String,
    pub state: PullState,
    /// The head branch.
    pub branch: String,
    pub added: u64,
    pub removed: u64,
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
