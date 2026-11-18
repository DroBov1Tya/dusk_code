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
