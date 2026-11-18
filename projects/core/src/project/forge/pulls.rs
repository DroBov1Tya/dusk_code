//! A project's pull requests: listing the open ones, one with its body and checks, and
//! a new one from the current branch.

use anyhow::{Context, Result, bail};
use dusk_protocol::project::{
    Check, CheckStage, CheckState, Project, PullRequest, PullRequestDetail,
};
use serde::Deserialize;

use super::gh::{PROGRAM, gh, link, plain, time};
use crate::project::git;

const MAX_TITLE_CHARS: usize = 300;
const MAX_BODY_CHARS: usize = 20_000;
const MAX_LISTED: usize = 50;
const LIST_FIELDS: &str =
    "number,title,author,headRefName,baseRefName,additions,deletions,isDraft,statusCheckRollup,url";

pub async fn list(project: &Project) -> Result<Vec<PullRequest>> {
    let limit = format!("--limit={MAX_LISTED}");
    let fields = format!("--json={LIST_FIELDS}");
    let text = gh(
        PROGRAM,
        &project.dir,
        &["pr", "list", "--state=open", &limit, &fields],
    )
    .await?;
    let raw: Vec<RawPull> = serde_json::from_str(&text).context("gh gave a list it cannot read")?;
    Ok(raw
        .into_iter()
        .take(MAX_LISTED)
        .map(RawPull::shown)
        .collect())
}

pub async fn view(project: &Project, number: u64) -> Result<PullRequestDetail> {
    view_of(project, Some(number)).await
}

/// A pull request from the current branch into `base`, or the repository's default
/// branch; answers with it as GitHub now has it.
pub async fn create(
    project: &Project,
    title: &str,
    body: &str,
    base: Option<&str>,
) -> Result<PullRequestDetail> {
    let title = format!("--title={}", git::check_message(title)?);
    let body = format!("--body={}", body.trim());
    if body.chars().count() > MAX_BODY_CHARS {
        bail!("the description is at most {MAX_BODY_CHARS} characters");
    }
    let base = base
        .map(|base| git::check_branch(base).map(|base| format!("--base={base}")))
        .transpose()?;
    let mut args = vec!["pr", "create", &title, &body];
    args.extend(base.as_deref());
    gh(PROGRAM, &project.dir, &args).await?;
    view_of(project, None).await
}

/// One pull request with its body and checks; the current branch's when `number` is none.
async fn view_of(project: &Project, number: Option<u64>) -> Result<PullRequestDetail> {
    let fields = format!("--json={LIST_FIELDS},body,state");
    let number = number.map(|number| number.to_string());
    let mut args = vec!["pr", "view"];
    args.extend(number.as_deref());
    args.push(&fields);
    let text = gh(PROGRAM, &project.dir, &args).await?;
    let raw: RawPull =
        serde_json::from_str(&text).context("gh gave a pull request it cannot read")?;
    let checks = raw
        .status_check_rollup
        .iter()
        .take(MAX_LISTED)
        .map(RawCheck::shown)
        .collect();
    let state = plain(&raw.state, 20);
    let body = plain(&raw.body, MAX_BODY_CHARS);
    Ok(PullRequestDetail {
        pull: raw.shown(),
        state,
        body,
        checks,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPull {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    author: Option<RawAuthor>,
    #[serde(default)]
    head_ref_name: String,
    #[serde(default)]
    base_ref_name: String,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    status_check_rollup: Vec<RawCheck>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    state: String,
}

#[derive(Deserialize)]
struct RawAuthor {
    #[serde(default)]
    login: String,
}

/// A check run (`name`, `status`, `conclusion`, `detailsUrl`) or a commit status
/// (`context`, `state`, `targetUrl`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RawCheck {
    #[serde(default)]
    name: String,
    #[serde(default)]
    context: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    workflow_name: String,
    #[serde(default)]
    details_url: String,
    #[serde(default)]
    target_url: String,
    #[serde(default)]
    started_at: String,
    #[serde(default)]
    completed_at: String,
}

impl RawCheck {
    fn name(&self) -> &str {
        if self.name.is_empty() {
            &self.context
        } else {
            &self.name
        }
    }

    pub(super) fn state(&self) -> CheckState {
        self.stage().state()
    }

    pub(super) fn stage(&self) -> CheckStage {
        let failed = [
            "FAILURE",
            "CANCELLED",
            "TIMED_OUT",
            "ACTION_REQUIRED",
            "ERROR",
            "STARTUP_FAILURE",
        ];
        let waiting = ["QUEUED", "WAITING", "PENDING", "REQUESTED"];
        let (status, conclusion) = (self.status.as_str(), self.conclusion.as_str());
        if failed.contains(&conclusion) || failed.contains(&self.state.as_str()) {
            CheckStage::Failed
        } else if conclusion == "SKIPPED" || conclusion == "NEUTRAL" {
            CheckStage::Skipped
        } else if self.state == "SUCCESS" || (status == "COMPLETED" && !conclusion.is_empty()) {
            CheckStage::Passed
        } else if waiting.contains(&status) || self.state == "EXPECTED" {
            CheckStage::Queued
        } else {
            CheckStage::Running
        }
    }

    /// The check as shown: its text plain and on one line, its link https only.
    pub(super) fn shown(&self) -> Check {
        let url = if self.details_url.is_empty() {
            &self.target_url
        } else {
            &self.details_url
        };
        Check {
            name: plain(self.name(), MAX_TITLE_CHARS).replace('\n', " "),
            workflow: plain(&self.workflow_name, 200).replace('\n', " "),
            stage: self.stage(),
            url: link(url),
            started: time(&self.started_at),
            finished: time(&self.completed_at),
        }
    }
}

impl RawPull {
    fn shown(self) -> PullRequest {
        let states: Vec<CheckState> = self
            .status_check_rollup
            .iter()
            .map(RawCheck::state)
            .collect();
        let checks = if states.is_empty() {
            None
        } else if states.contains(&CheckState::Failing) {
            Some(CheckState::Failing)
        } else if states.contains(&CheckState::Pending) {
            Some(CheckState::Pending)
        } else {
            Some(CheckState::Passing)
        };
        PullRequest {
            number: self.number,
            title: plain(&self.title, MAX_TITLE_CHARS),
            author: plain(&self.author.map(|a| a.login).unwrap_or_default(), 100),
            branch: plain(&self.head_ref_name, 250),
            base: plain(&self.base_ref_name, 250),
            added: self.additions,
            removed: self.deletions,
            draft: self.is_draft,
            checks,
            url: link(&self.url),
        }
    }
}
