//! A project's repository at a glance: the work tree's diff stat, the current branch,
//! and the pull requests that matter to it with their checks counted. GitHub's part is
//! left out, with a reason, where `gh` cannot be used or the remote is not GitHub's.

use std::path::Path;

use anyhow::{Context, Result};
use dusk_protocol::project::{
    CheckState, Checks, Project, PullState, PullStatus, RepositoryStatus,
};
use serde::Deserialize;

use super::{
    gh::{MISSING, PROGRAM, link, output, plain, ready},
    pulls::RawCheck,
};
use crate::project::{diff, git};

/// The current branch's pull requests, newest first, merged and closed ones too.
const MAX_BRANCH_PULLS: usize = 5;
/// The project's other open pull requests.
const MAX_OPEN_PULLS: usize = 10;
/// More checks than this on one pull request are not counted.
const MAX_CHECKS: usize = 500;
const MAX_TITLE_CHARS: usize = 300;
const FIELDS: &str = "number,title,state,isDraft,isCrossRepository,headRefName,additions,\
                      deletions,url,statusCheckRollup";

/// A status as read, and what the tracker needs besides it.
pub struct Reading {
    pub status: RepositoryStatus,
    /// The first `mine` pull requests are the current branch's.
    pub mine: usize,
    /// GitHub was asked and did not answer usably: worth asking again later, and the
    /// pull requests known before are better than none.
    pub trouble: bool,
}

/// Never an error: what cannot be read is left out, with its reason.
pub async fn read(project: &Project) -> Reading {
    read_with(PROGRAM, &project.dir).await
}

async fn read_with(program: &str, dir: &Path) -> Reading {
    let mut reading = Reading {
        status: RepositoryStatus::default(),
        mine: 0,
        trouble: false,
    };
    let Ok(top) = git::git(dir, &["rev-parse", "--show-toplevel"]).await else {
        reading.status.unavailable = Some("Not a git repository".to_owned());
        return reading;
    };
    let status = &mut reading.status;
    status.repository = Path::new(top.trim())
        .file_name()
        .map(|name| plain(&name.to_string_lossy(), 200))
        .unwrap_or_default();
    status.branch = git::git(dir, &["branch", "--show-current"])
        .await
        .ok()
        .map(|branch| branch.trim().to_owned())
        .filter(|branch| !branch.is_empty());
    status.stat = diff::stat(dir, &git::changes(dir).await).await;
    if !on_github(dir).await {
        status.unavailable = Some("Not a GitHub repository".to_owned());
        return reading;
    }
    if ready(program, dir).await.is_err() {
        status.unavailable = Some(MISSING.to_owned());
        return reading;
    }
    match pulls(program, dir, status.branch.as_deref()).await {
        Ok((pulls, mine)) => {
            status.pulls = pulls;
            reading.mine = mine;
        }
        Err(error) => {
            tracing::debug!("pull requests not read: {error:#}");
            status.unavailable = Some("GitHub did not answer".to_owned());
            reading.trouble = true;
        }
    }
    reading
}

/// Whether `origin` is on github.com; another host is not asked.
async fn on_github(dir: &Path) -> bool {
    git::git(dir, &["remote", "get-url", "origin"])
        .await
        .ok()
        .and_then(|remote| git::web_page(remote.trim()))
        .is_some_and(|page| page.starts_with("https://github.com/"))
}

/// The branch's pull requests, then the other open ones; and how many are the branch's.
async fn pulls(
    program: &str,
    dir: &Path,
    branch: Option<&str>,
) -> Result<(Vec<PullStatus>, usize)> {
    let fields = format!("--json={FIELDS}");
    let mut pulls = Vec::new();
    if let Some(branch) = branch.and_then(|branch| git::check_branch(branch).ok()) {
        let head = format!("--head={branch}");
        let limit = format!("--limit={MAX_BRANCH_PULLS}");
        let args = ["pr", "list", "--state=all", &head, &limit, &fields];
        let text = output(program, dir, &args).await?;
        // A fork's branch of the same name is someone else's.
        pulls = parse(&text, MAX_BRANCH_PULLS)?
            .into_iter()
            .filter(|(_, cross)| !cross)
            .map(|(pull, _)| pull)
            .filter(|pull| pull.branch == branch)
            .collect();
    }
    let mine = pulls.len();
    let limit = format!("--limit={MAX_OPEN_PULLS}");
    let args = ["pr", "list", "--state=open", &limit, &fields];
    let text = output(program, dir, &args).await?;
    for (pull, _) in parse(&text, MAX_OPEN_PULLS)? {
        if !pulls.iter().any(|known| known.number == pull.number) {
            pulls.push(pull);
        }
    }
    Ok((pulls, mine))
}

/// What `gh pr list --json` printed, at most `max` of it; each with whether it comes
/// from a fork.
fn parse(text: &str, max: usize) -> Result<Vec<(PullStatus, bool)>> {
    let raw: Vec<RawPull> = serde_json::from_str(text).context("gh gave a list it cannot read")?;
    Ok(raw
        .into_iter()
        .take(max)
        .map(|raw| {
            let cross = raw.is_cross_repository;
            (raw.shown(), cross)
        })
        .collect())
}

/// Every check counted; the state is the worst of them.
pub(super) fn roll_up(checks: &[RawCheck]) -> Checks {
    let mut counted = Checks::default();
    for check in checks.iter().take(MAX_CHECKS) {
        counted.total += 1;
        match check.state() {
            CheckState::Passing => counted.passed += 1,
            CheckState::Failing => counted.failed += 1,
            CheckState::Pending => counted.running += 1,
        }
    }
    counted.state = if counted.total == 0 {
        None
    } else if counted.failed > 0 {
        Some(CheckState::Failing)
    } else if counted.running > 0 {
        Some(CheckState::Pending)
    } else {
        Some(CheckState::Passing)
    };
    counted
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPull {
    number: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    is_cross_repository: bool,
    #[serde(default)]
    head_ref_name: String,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    url: String,
    #[serde(default)]
    status_check_rollup: Vec<RawCheck>,
}

impl RawPull {
    fn shown(self) -> PullStatus {
        let state = match self.state.as_str() {
            "MERGED" => PullState::Merged,
            "CLOSED" => PullState::Closed,
            _ if self.is_draft => PullState::Draft,
            _ => PullState::Open,
        };
        PullStatus {
            number: self.number,
            title: plain(&self.title, MAX_TITLE_CHARS).replace('\n', " "),
            state,
            branch: plain(&self.head_ref_name, 250),
            added: self.additions,
            removed: self.deletions,
            url: link(&self.url),
            checks: roll_up(&self.status_check_rollup),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(json: &str) -> RawCheck {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn checks_roll_up_to_the_worst() {
        let passed = check(r#"{"name":"a","status":"COMPLETED","conclusion":"SUCCESS"}"#);
        let skipped = check(r#"{"name":"b","status":"COMPLETED","conclusion":"SKIPPED"}"#);
        let running = check(r#"{"name":"c","status":"IN_PROGRESS","conclusion":""}"#);
        let queued = check(r#"{"context":"d","state":"PENDING"}"#);
        let failed = check(r#"{"name":"e","status":"COMPLETED","conclusion":"TIMED_OUT"}"#);
        let errored = check(r#"{"context":"f","state":"ERROR"}"#);

        assert_eq!(roll_up(&[]), Checks::default());
        let all_passed = roll_up(&[passed, skipped]);
        assert_eq!((all_passed.total, all_passed.passed), (2, 2));
        assert_eq!(all_passed.state, Some(CheckState::Passing));
        let passed = check(r#"{"name":"a","status":"COMPLETED","conclusion":"SUCCESS"}"#);
        let some_running = roll_up(&[passed, running, queued]);
        assert_eq!(
            (some_running.running, some_running.state),
            (2, Some(CheckState::Pending))
        );
        let running = check(r#"{"name":"c","status":"QUEUED"}"#);
        let one_failed = roll_up(&[running, failed, errored]);
        assert_eq!(
            (one_failed.total, one_failed.failed, one_failed.running),
            (3, 2, 1)
        );
        assert_eq!(one_failed.state, Some(CheckState::Failing));
    }

    #[test]
    fn a_list_from_gh_is_read_into_plain_capped_rows() {
        let text = r#"[
          {"number":12,"title":"Fix\u001b[31m it\nnow","state":"OPEN","isDraft":true,
           "isCrossRepository":false,"headRefName":"fix","additions":10,"deletions":7,
           "url":"https://github.com/o/r/pull/12","unknown":{"x":1},
           "statusCheckRollup":[{"__typename":"CheckRun","name":"ci","status":"COMPLETED",
             "conclusion":"SUCCESS","detailsUrl":"https://x"}]},
          {"number":9,"state":"MERGED","url":"javascript:alert(1)"},
          {"number":3,"state":"CLOSED","isCrossRepository":true}
        ]"#;
        let rows = parse(text, 10).unwrap();
        let (first, cross) = &rows[0];
        assert!(!cross);
        assert_eq!(first.title, "Fix[31m it now");
        assert_eq!(first.state, PullState::Draft);
        assert_eq!((first.added, first.removed), (10, 7));
        assert_eq!(first.url, "https://github.com/o/r/pull/12");
        assert_eq!(first.checks.state, Some(CheckState::Passing));
        assert_eq!(rows[1].0.state, PullState::Merged);
        assert_eq!(rows[1].0.url, "", "only https links are kept");
        assert_eq!(rows[1].0.checks.state, None);
        assert!(rows[2].1);
        assert_eq!(rows[2].0.state, PullState::Closed);
    }

    #[test]
    fn malformed_or_oversized_output_is_refused_or_capped() {
        for bad in [
            "",
            "{",
            "{}",
            r#"[{"title":"no number"}]"#,
            r#"[{"number":-1}]"#,
        ] {
            assert!(parse(bad, 10).is_err(), "{bad}");
        }
        let many = format!(
            "[{}]",
            (0..1000)
                .map(|n| format!(r#"{{"number":{n},"title":"{}"}}"#, "x".repeat(400)))
                .collect::<Vec<_>>()
                .join(",")
        );
        let rows = parse(&many, MAX_OPEN_PULLS).unwrap();
        assert_eq!(rows.len(), MAX_OPEN_PULLS);
        assert_eq!(rows[0].0.title.chars().count(), MAX_TITLE_CHARS + 3);
        let checks: Vec<String> = (0..2000)
            .map(|_| r#"{"name":"a","status":"QUEUED"}"#.to_owned())
            .collect();
        let crowded = format!(
            r#"[{{"number":1,"statusCheckRollup":[{}]}}]"#,
            checks.join(",")
        );
        assert_eq!(
            parse(&crowded, 1).unwrap()[0].0.checks.total,
            MAX_CHECKS as u32
        );
    }

    #[tokio::test]
    async fn outside_a_repository_github_is_never_asked() {
        let dir = crate::testing::temp_dir("forge_status");
        std::fs::create_dir_all(&dir).unwrap();
        // A program that does not exist: asking it would be a failure, not a reason.
        let reading = read_with("/nonexistent/gh", &dir).await;
        assert_eq!(
            reading.status.unavailable.as_deref(),
            Some("Not a git repository")
        );
        assert!(!reading.trouble);
    }
}
