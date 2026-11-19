//! The forge contract, version 1: a service that hosts git repositories with pull
//! requests and CI. The core sends these arguments and reads these answers; the plugin
//! maps its vendor's states onto them, so the core never sees a vendor string. Every
//! field of an answer but a pull's `number` may be left out.

use serde::{Deserialize, Serialize};

use crate::project::{CheckStage, PullState};

/// The versions the core speaks, newest last.
pub const VERSIONS: [u32; 1] = [1];

/// `status`: the pull requests that matter to a branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusArgs {
    pub host: String,
    /// `owner/repo`, as the remote names it.
    pub repository: String,
    /// `None` on a detached head.
    pub branch: Option<String>,
    pub limits: Limits,
}

/// What the core keeps; a program may stop reading at these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub branch_pulls: u32,
    pub open_pulls: u32,
    pub checks: u32,
}

/// `pull`: one pull request by number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullArgs {
    pub host: String,
    pub repository: String,
    pub number: u64,
}

/// `create_pull`: a pull request from a branch the user already pushed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatePullArgs {
    pub host: String,
    pub repository: String,
    pub head: String,
    /// `None`: the repository's default branch.
    pub base: Option<String>,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusAnswer {
    /// What the plugin calls the forge.
    #[serde(default)]
    pub name: String,
    /// Every pull's page is this plus its number.
    #[serde(default)]
    pub pull_prefix: String,
    /// The branch's pull requests in any state, newest first.
    #[serde(default)]
    pub branch_pulls: Vec<Pull>,
    /// The repository's open ones, most recently updated first.
    #[serde(default)]
    pub open_pulls: Vec<Pull>,
}

/// The answer of `pull` and `create_pull`; `None` when there is none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullAnswer {
    #[serde(default)]
    pub pull: Option<Pull>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pull {
    pub number: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub state: State,
    /// The head branch.
    #[serde(default)]
    pub branch: String,
    /// The head lives in a fork.
    #[serde(default)]
    pub from_fork: bool,
    #[serde(default)]
    pub author: String,
    /// Unix seconds.
    #[serde(default)]
    pub created: Option<i64>,
    #[serde(default)]
    pub updated: Option<i64>,
    #[serde(default)]
    pub added: u64,
    #[serde(default)]
    pub removed: u64,
    #[serde(default)]
    pub files: Option<u64>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub checks_url: String,
    #[serde(default)]
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub workflow: String,
    #[serde(default)]
    pub stage: Stage,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub started: Option<i64>,
    #[serde(default)]
    pub finished: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    #[default]
    Open,
    Draft,
    Merged,
    Closed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    #[default]
    Queued,
    Running,
    Passed,
    Skipped,
    Failed,
}

impl From<State> for PullState {
    fn from(state: State) -> Self {
        match state {
            State::Open => PullState::Open,
            State::Draft => PullState::Draft,
            State::Merged => PullState::Merged,
            State::Closed => PullState::Closed,
        }
    }
}

impl From<Stage> for CheckStage {
    fn from(stage: Stage) -> Self {
        match stage {
            Stage::Queued => CheckStage::Queued,
            Stage::Running => CheckStage::Running,
            Stage::Passed => CheckStage::Passed,
            Stage::Skipped => CheckStage::Skipped,
            Stage::Failed => CheckStage::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_pull_parses_and_a_bare_one_defaults() {
        let full = r#"{"number":12,"title":"Fix","state":"draft","branch":"x","from_fork":false,
            "author":"o","created":1,"updated":2,"added":10,"removed":2,"files":null,
            "url":"https://h/o/r/pull/12","checks_url":"","future":1,
            "checks":[{"name":"build","workflow":"CI","stage":"running","url":"","started":1,"finished":null}]}"#;
        let pull: Pull = serde_json::from_str(full).unwrap();
        assert_eq!(pull.state, State::Draft);
        assert_eq!(pull.checks[0].stage, Stage::Running);
        assert_eq!(CheckStage::from(pull.checks[0].stage), CheckStage::Running);
        assert_eq!(PullState::from(pull.state), PullState::Draft);
        let bare: Pull = serde_json::from_str(r#"{"number":3}"#).unwrap();
        assert_eq!(bare.number, 3);
        assert!(bare.checks.is_empty());
        assert!(serde_json::from_str::<Pull>("{}").is_err());
        let answer: StatusAnswer = serde_json::from_str("{}").unwrap();
        assert!(answer.name.is_empty() && answer.open_pulls.is_empty());
    }
}
