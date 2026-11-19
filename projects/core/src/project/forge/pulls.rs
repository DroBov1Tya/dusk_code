//! A new pull request from the current branch, made by the plugin that serves the
//! remote's host or by `gh`, and the raw check a pull request carries.

use anyhow::{Result, anyhow, bail};
use dusk_protocol::{
    plugin::contract::{
        ContractError, ErrorKind,
        forge::{self as contract, CreatePullArgs, Stage},
    },
    project::{Project, PullStatus},
};
use serde::Deserialize;

use super::{
    Forges, Remote, checked,
    gh::{PROGRAM, gh, link, plain, time},
};
use crate::{plugins::contract::Served, project::git};

const MAX_TITLE_CHARS: usize = 300;
const MAX_BODY_CHARS: usize = 20_000;

/// A pull request from the current branch into `base`, or the repository's default
/// branch; made by the plugin that serves the remote's host while the switch is on, else
/// by `gh`. Answers with it as the forge now has it.
pub async fn create(
    project: &Project,
    title: &str,
    body: &str,
    base: Option<&str>,
    forges: &Forges,
) -> Result<PullStatus> {
    let title = git::check_message(title)?;
    let body = body.trim();
    if body.chars().count() > MAX_BODY_CHARS {
        bail!("the description is at most {MAX_BODY_CHARS} characters");
    }
    let base = base.map(git::check_branch).transpose()?;
    // Named, so that a detached head is refused here and not guessed by `gh`.
    let branch = git::git(&project.dir, &["branch", "--show-current"]).await?;
    let branch = branch.trim();
    if branch.is_empty() {
        bail!("a detached head has no pull request; switch to a branch first");
    }
    let branch = git::check_branch(branch)?;
    if let Some(remote) = Remote::of(&project.dir).await
        && let Some(served) = forges.route(&remote.host)
    {
        let args = CreatePullArgs {
            host: remote.host.clone(),
            repository: remote.path.clone(),
            head: branch.to_owned(),
            base: base.map(str::to_owned),
            title: title.to_owned(),
            body: body.to_owned(),
        };
        return ask(project, &served, forges, &remote, &args).await;
    }
    let title = format!("--title={title}");
    let body = format!("--body={body}");
    let base = base.map(|base| format!("--base={base}"));
    let head = format!("--head={branch}");
    let mut args = vec!["pr", "create", &head, &title, &body];
    args.extend(base.as_deref());
    gh(PROGRAM, &project.dir, &args).await?;
    super::status::read_current(project).await
}

/// `create_pull` of the plugin that serves the remote; its answer checked for the shape
/// of a pull only.
async fn ask(
    project: &Project,
    served: &Served,
    forges: &Forges,
    remote: &Remote,
    args: &CreatePullArgs,
) -> Result<PullStatus> {
    let name = &served.plugin;
    let ok = forges
        .call(served, "create_pull", &project.dir, args)
        .await
        .map_err(|error| refusal(&error, name))?;
    checked::made(ok, &remote.host).map_err(|error| {
        tracing::debug!(
            "{name} made a pull request answered amiss: {}",
            error.message
        );
        // It may well have been made: the user is told to look rather than to try again.
        anyhow!("{name} answered with no pull request; it may have been made, check {name}")
    })
}

/// What the user reads when the plugin `name` did not make it: a refusal says why in the
/// forge's words; anything else as the status would say it.
fn refusal(error: &ContractError, name: &str) -> anyhow::Error {
    match error.kind {
        ErrorKind::Refused if !error.message.is_empty() => anyhow!("{}", error.message),
        ErrorKind::Refused => anyhow!("{name} refused it"),
        ErrorKind::SignedOut if !error.hint.is_empty() => {
            anyhow!("{name} is not signed in: {}", error.hint)
        }
        ErrorKind::SignedOut => anyhow!("{name} is not signed in"),
        _ => anyhow!("{}", checked::note(error, name).0),
    }
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

    /// GitHub's states mapped onto the contract's stages, as the GitHub plugin does.
    fn stage(&self) -> Stage {
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
            Stage::Failed
        } else if conclusion == "SKIPPED" || conclusion == "NEUTRAL" {
            Stage::Skipped
        } else if self.state == "SUCCESS" || (status == "COMPLETED" && !conclusion.is_empty()) {
            Stage::Passed
        } else if waiting.contains(&status) || self.state == "EXPECTED" {
            Stage::Queued
        } else {
            Stage::Running
        }
    }

    /// The check as a plugin would answer it: its text plain and on one line, its link
    /// https only.
    pub(super) fn neutral(&self) -> contract::Check {
        let url = if self.details_url.is_empty() {
            &self.target_url
        } else {
            &self.details_url
        };
        contract::Check {
            name: plain(self.name(), MAX_TITLE_CHARS).replace('\n', " "),
            workflow: plain(&self.workflow_name, 200).replace('\n', " "),
            stage: self.stage(),
            url: link(url),
            started: time(&self.started_at),
            finished: time(&self.completed_at),
        }
    }
}
