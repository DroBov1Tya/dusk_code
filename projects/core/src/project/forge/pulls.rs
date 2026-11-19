//! A new pull request from the current branch, and the raw check a pull request carries.

use anyhow::{Result, bail};
use dusk_protocol::project::{Check, CheckStage, CheckState, Project, PullStatus};
use serde::Deserialize;

use super::gh::{PROGRAM, gh, link, plain, time};
use crate::project::git;

const MAX_TITLE_CHARS: usize = 300;
const MAX_BODY_CHARS: usize = 20_000;

/// A pull request from the current branch into `base`, or the repository's default
/// branch; answers with it as the forge now has it.
pub async fn create(
    project: &Project,
    title: &str,
    body: &str,
    base: Option<&str>,
) -> Result<PullStatus> {
    let title = format!("--title={}", git::check_message(title)?);
    let body = body.trim();
    if body.chars().count() > MAX_BODY_CHARS {
        bail!("the description is at most {MAX_BODY_CHARS} characters");
    }
    let body = format!("--body={body}");
    let base = base
        .map(|base| git::check_branch(base).map(|base| format!("--base={base}")))
        .transpose()?;
    // Named, so that a detached head is refused here and not guessed by `gh`.
    let branch = git::git(&project.dir, &["branch", "--show-current"]).await?;
    let branch = branch.trim();
    if branch.is_empty() {
        bail!("a detached head has no pull request; switch to a branch first");
    }
    let head = format!("--head={}", git::check_branch(branch)?);
    let mut args = vec!["pr", "create", &head, &title, &body];
    args.extend(base.as_deref());
    gh(PROGRAM, &project.dir, &args).await?;
    super::status::read_current(project).await
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
