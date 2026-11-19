//! A project's pull requests: listing the open ones, one with its body and checks, and
//! a new one from the current branch, made by the plugin that serves the remote's host
//! or by `gh` for github.com.

use anyhow::{Context, Result, bail};
use dusk_protocol::{
    plugin::contract::{
        ContractError, ErrorKind,
        forge::{self as contract, CreatePullArgs, Limits, Stage, StatusArgs},
    },
    project::{
        Check, CheckStage, CheckState, Project, PullRequest, PullRequestDetail, PullState,
        PullStatus,
    },
};
use serde::Deserialize;

use super::{
    Forges, Remote, checked,
    gh::{PROGRAM, gh, link, plain, time},
};
use crate::{plugins::contract::Served, project::git};

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
    view_of(
        project,
        &["pr".to_owned(), "view".to_owned(), number.to_string()],
    )
    .await
}

/// A pull request from the current branch into `base`, or the repository's default
/// branch; answers with it as the forge now has it, when it is one of the remote's own.
pub async fn create(
    project: &Project,
    title: &str,
    body: &str,
    base: Option<&str>,
    forges: &Forges,
) -> Result<PullRequestDetail> {
    let title = git::check_message(title)?;
    let body = body.trim();
    if body.chars().count() > MAX_BODY_CHARS {
        bail!("the description is at most {MAX_BODY_CHARS} characters");
    }
    let base = base.map(git::check_branch).transpose()?;
    let Some(remote) = Remote::of(&project.dir).await else {
        bail!("the repository has no remote on a forge");
    };
    if let Some(served) = forges.route(&remote.host) {
        let made = ask(project, &served, forges, &remote, title, body, base).await?;
        return Ok(detail(made, base, body));
    }
    if remote.host == "github.com" && remote.path.split('/').count() != 2 {
        bail!(
            "{} is not an owner/repo repository of github.com",
            remote.path
        );
    }
    if remote.host != "github.com" {
        let plugins = if forges.on {
            ", and no plugin that is on serves it"
        } else {
            ""
        };
        bail!(
            "{} is not served: only github.com is built in{plugins}",
            remote.host
        );
    }
    let head = head(project).await?;
    let args = create_args(&remote, &head, title, body, base);
    let printed = gh(PROGRAM, &project.dir, &as_strs(&args)).await?;
    // Read back by the number in the page `gh` printed, never by the branch: a branch of
    // digits would read as another pull's number.
    let Some(number) = made_number(&printed, &remote) else {
        bail!(
            "GitHub answered with no pull request of {}; it may have been made, check GitHub",
            remote.path
        );
    };
    let made = view_of(project, &view_args(number, &remote)).await?;
    // Named by `--repo`, so made on the remote's own; checked still, in case `gh` did not.
    let page = format!("https://github.com/{}/pull/{number}", remote.path);
    if made.pull.number != number || !made.pull.url.eq_ignore_ascii_case(&page) {
        bail!("the pull request made is not one of {}", remote.path);
    }
    Ok(made)
}

/// The number of the pull request whose page `gh pr create` printed last, when it is one
/// of the remote's own.
fn made_number(printed: &str, remote: &Remote) -> Option<u64> {
    let page = printed
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let prefix = format!("https://github.com/{}/pull/", remote.path);
    let number = page
        .get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(&prefix))
        .and_then(|_| page.get(prefix.len()..))?;
    let fine =
        !number.is_empty() && number.len() <= 12 && number.bytes().all(|b| b.is_ascii_digit());
    fine.then(|| number.parse().ok())
        .flatten()
        .filter(|n| *n > 0)
}

/// `gh pr view` of pull `number` of the remote's own repository.
fn view_args(number: u64, remote: &Remote) -> Vec<String> {
    vec![
        "pr".to_owned(),
        "view".to_owned(),
        number.to_string(),
        repo_arg(remote),
    ]
}

/// `create_pull` of the plugin that serves the remote, checked against the prefix its
/// status names.
async fn ask(
    project: &Project,
    served: &Served,
    forges: &Forges,
    remote: &Remote,
    title: &str,
    body: &str,
    base: Option<&str>,
) -> Result<PullStatus> {
    let plugin = &served.plugin;
    let head = head(project).await?;
    let head = head.as_str();
    let dir = &project.dir;
    let status = StatusArgs {
        host: remote.host.clone(),
        repository: remote.path.clone(),
        branch: Some(head.to_owned()),
        limits: Limits {
            branch_pulls: 1,
            open_pulls: 1,
            checks: 1,
        },
    };
    let ok = forges
        .call(served, "status", dir, &status)
        .await
        .map_err(|error| refusal(&error, plugin, false))?;
    let Some((status, _)) = checked::status(ok, remote, Some(head), plugin) else {
        bail!("{plugin} could not be read");
    };
    let name = &status.name;
    let args = CreatePullArgs {
        host: remote.host.clone(),
        repository: remote.path.clone(),
        head: head.to_owned(),
        base: base.map(str::to_owned),
        title: title.to_owned(),
        body: body.to_owned(),
    };
    let ok = forges
        .call(served, "create_pull", dir, &args)
        .await
        .map_err(|error| refusal(&error, name, true))?;
    match checked::pull(ok, &status.pull_prefix, &remote.host, None) {
        Ok(Some(pull)) => Ok(pull),
        // It may well have been made: the user is told to look rather than to try again.
        answer => {
            if let Err(error) = answer {
                tracing::debug!(
                    "{name} made a pull request answered amiss: {}",
                    error.message
                );
            }
            bail!(
                "{name} answered with no pull request of {}; it may have been made, check {name}",
                remote.path
            )
        }
    }
}

/// The current branch, the head of a new pull request.
async fn head(project: &Project) -> Result<String> {
    let head = git::git(&project.dir, &["branch", "--show-current"]).await?;
    let head = head.trim();
    if head.is_empty() {
        bail!("a detached head has no branch to propose");
    }
    Ok(git::check_branch(head)?.to_owned())
}

/// `--repo` naming the remote's own repository, so `gh` never picks another, such as
/// the one a fork came from.
fn repo_arg(remote: &Remote) -> String {
    format!("--repo={}/{}", remote.host, remote.path)
}

/// `gh pr create` from `head` on the remote's own repository.
fn create_args(
    remote: &Remote,
    head: &str,
    title: &str,
    body: &str,
    base: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "pr".to_owned(),
        "create".to_owned(),
        repo_arg(remote),
        format!("--head={head}"),
        format!("--title={title}"),
        format!("--body={body}"),
    ];
    args.extend(base.map(|base| format!("--base={base}")));
    args
}

fn as_strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

/// What the user reads when the forge `name` did not make it: a refusal of `create_pull`
/// says why in the forge's words; anything else as the status would say it.
fn refusal(error: &ContractError, name: &str, create: bool) -> anyhow::Error {
    match error.kind {
        ErrorKind::Refused if create && !error.message.is_empty() => {
            anyhow::anyhow!("{}", error.message)
        }
        ErrorKind::Refused if create => anyhow::anyhow!("{name} refused it"),
        ErrorKind::SignedOut if !error.hint.is_empty() => {
            anyhow::anyhow!("{name} is not signed in: {}", error.hint)
        }
        ErrorKind::SignedOut => anyhow::anyhow!("{name} is not signed in"),
        _ => anyhow::anyhow!("{}", checked::note(error, name).0),
    }
}

/// A pull a plugin made, in the shape the request answers with.
fn detail(pull: PullStatus, base: Option<&str>, body: &str) -> PullRequestDetail {
    let state = match pull.state {
        PullState::Open | PullState::Draft => "OPEN",
        PullState::Merged => "MERGED",
        PullState::Closed => "CLOSED",
    };
    PullRequestDetail {
        pull: PullRequest {
            number: pull.number,
            title: pull.title,
            author: pull.author,
            branch: pull.branch,
            base: base.unwrap_or_default().to_owned(),
            added: pull.added,
            removed: pull.removed,
            draft: pull.state == PullState::Draft,
            checks: pull.checks.state,
            url: pull.url,
        },
        state: state.to_owned(),
        body: plain(body, MAX_BODY_CHARS),
        checks: pull.checks.list,
    }
}

/// One pull request with its body and checks, as `gh` is asked by `view` (`pr view`
/// and what names it).
async fn view_of(project: &Project, view: &[String]) -> Result<PullRequestDetail> {
    let fields = format!("--json={LIST_FIELDS},body,state");
    let mut args = as_strs(view);
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

    fn state(&self) -> CheckState {
        CheckStage::from(self.stage()).state()
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

    fn shown(&self) -> Check {
        let check = self.neutral();
        Check {
            name: check.name,
            workflow: check.workflow,
            stage: check.stage.into(),
            url: check.url,
            started: check.started,
            finished: check.finished,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gh_creates_only_on_the_remotes_own_repository() {
        let remote = Remote {
            host: "github.com".into(),
            path: "o/r".into(),
        };
        let args = create_args(&remote, "x", "Fix", "b", Some("main"));
        assert_eq!(
            args,
            [
                "pr",
                "create",
                "--repo=github.com/o/r",
                "--head=x",
                "--title=Fix",
                "--body=b",
                "--base=main"
            ]
        );
        assert_eq!(create_args(&remote, "x", "Fix", "", None).len(), 6);

        // A branch of digits is never read back as a pull's number: the page printed is.
        let args = create_args(&remote, "123", "Fix", "", None);
        assert_eq!(args[3], "--head=123");
        let number = made_number("https://github.com/o/r/pull/57\n", &remote).unwrap();
        assert_eq!(number, 57);
        assert_eq!(
            view_args(number, &remote),
            ["pr", "view", "57", "--repo=github.com/o/r"]
        );
        assert_eq!(
            made_number("Creating...\n\nhttps://github.com/O/R/pull/9\n", &remote),
            Some(9)
        );
        for other in [
            "https://github.com/parent/r/pull/57",
            "https://github.com/o/r/pull/57/files",
            "https://github.com/o/r/pull/",
            "",
        ] {
            assert_eq!(made_number(other, &remote), None, "{other}");
        }
    }
}
