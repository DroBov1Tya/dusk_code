//! A project's pull requests on GitHub, through the GitHub CLI (`gh`) the user installed
//! and signed in to. The core holds no token: `gh` keeps its own. Arguments are a vector
//! with every value in `--flag=value` form; what `gh` prints is untrusted JSON, parsed
//! into fixed shapes, its text made plain and capped.

use std::{path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use dusk_protocol::project::{Check, CheckState, Project, PullRequest, PullRequestDetail};
use serde::Deserialize;

use super::git;

const PROGRAM: &str = "gh";
const LIMIT: Duration = Duration::from_secs(60);
/// What is said when `gh` cannot be used, and nothing else.
pub const MISSING: &str = "GitHub CLI is not installed or signed in";
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
        .map(|check| Check {
            name: plain(check.name(), MAX_TITLE_CHARS),
            state: check.state(),
        })
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

/// Runs `gh` in `dir` once it is known to be there and signed in; an error carries
/// what it said, plain.
async fn gh(program: &str, dir: &Path, args: &[&str]) -> Result<String> {
    ready(program, dir).await?;
    let output = run(program, dir, args).await?;
    if !output.success {
        bail!("gh: {}", output.message());
    }
    Ok(output.stdout)
}

/// `gh` is installed and signed in; otherwise `MISSING`, whatever the cause.
async fn ready(program: &str, dir: &Path) -> Result<()> {
    match run(program, dir, &["auth", "status"]).await {
        Ok(output) if output.success => Ok(()),
        _ => bail!(MISSING),
    }
}

async fn run(program: &str, dir: &Path, args: &[&str]) -> Result<git::Output> {
    let mut command = git::command(program, dir);
    command
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GH_SPINNER_DISABLED", "1")
        .env("GH_PAGER", "cat")
        .env("NO_COLOR", "1");
    git::finish(command, LIMIT).await
}

/// Untrusted text as plain text: control characters out, capped.
fn plain(text: &str, max: usize) -> String {
    let mut out: String = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(max)
        .collect();
    if text.chars().count() > max {
        out.push_str("...");
    }
    out
}

/// An https link to GitHub, or nothing.
fn link(url: &str) -> String {
    let safe = url.starts_with("https://")
        && url.len() <= 500
        && url
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c));
    if safe { url.to_owned() } else { String::new() }
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

/// A check run (`name`, `status`, `conclusion`) or a commit status (`context`, `state`).
#[derive(Deserialize)]
struct RawCheck {
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
        let failed = [
            "FAILURE",
            "CANCELLED",
            "TIMED_OUT",
            "ACTION_REQUIRED",
            "ERROR",
            "STARTUP_FAILURE",
        ];
        if failed.contains(&self.conclusion.as_str()) || failed.contains(&self.state.as_str()) {
            CheckState::Failing
        } else if self.state == "SUCCESS"
            || (self.status == "COMPLETED" && !self.conclusion.is_empty())
        {
            CheckState::Passing
        } else {
            CheckState::Pending
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
