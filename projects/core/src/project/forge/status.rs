//! A project's repository at a glance: the work tree's diff stat, the current branch,
//! and the pull requests that matter to it with their checks counted. The forge's part
//! says why it is missing where `gh` cannot be used or the remote is not GitHub's.

use std::path::Path;

use anyhow::{Context, Result};
use dusk_protocol::project::{
    CheckStage, CheckState, Checks, Forge, ForgeSignIn, ForgeStatus, Project, PullState,
    PullStatus, RepositoryStatus,
};
use serde::Deserialize;

use super::{
    Remote,
    gh::{PROGRAM, gh, link, output, plain, ready, time},
    pulls::RawCheck,
};
use crate::project::{diff, git};

/// The plugin that answers for now: the built-in GitHub one.
const PLUGIN: &str = "github";

/// The current branch's pull requests, newest first, merged and closed ones too.
const MAX_BRANCH_PULLS: usize = 5;
/// The project's other open pull requests.
const MAX_OPEN_PULLS: usize = 10;
/// More checks than this on one pull request are not counted.
const MAX_CHECKS: usize = 500;
/// More checks than this on one pull request are counted but not listed.
const MAX_LISTED_CHECKS: usize = 50;
const MAX_TITLE_CHARS: usize = 300;
const FIELDS: &str = "number,title,state,isDraft,isCrossRepository,headRefName,author,\
                      createdAt,updatedAt,additions,deletions,changedFiles,url,statusCheckRollup";

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
    let Some(remote) = Remote::of(dir).await else {
        return reading;
    };
    if !is_github(&remote) {
        status.forge = Forge::Unserved {
            host: remote.host,
            offer: None,
        };
        return reading;
    }
    if ready(program, dir).await.is_err() {
        status.forge = Forge::SignIn(ForgeSignIn {
            host: remote.host,
            plugin: PLUGIN.to_owned(),
            hint: "gh auth login".to_owned(),
        });
        return reading;
    }
    let mut served = ForgeStatus {
        plugin: PLUGIN.to_owned(),
        name: "GitHub".to_owned(),
        repository: remote.path.clone(),
        pull_prefix: format!("https://github.com/{}/pull/", remote.path),
        pulls: Vec::new(),
        unavailable: None,
    };
    match pulls(program, dir, status.branch.as_deref()).await {
        Ok((pulls, mine)) => {
            served.pulls = pulls;
            reading.mine = mine;
        }
        Err(error) => {
            tracing::debug!("pull requests not read: {error:#}");
            served.unavailable = Some("GitHub did not answer".to_owned());
            reading.trouble = true;
        }
    }
    status.forge = Forge::Served(Box::new(served));
    reading
}

/// The one forge the core asks for now: github.com, `owner/repo`; another host or a
/// deeper path is not asked.
fn is_github(remote: &Remote) -> bool {
    remote.host == "github.com" && remote.path.split('/').count() == 2
}

/// Pull request `number` of the project's own GitHub repository, asked of that
/// repository by name; `None` when it has none such or GitHub cannot be asked.
pub(super) async fn read_one(project: &Project, number: u64) -> Option<PullStatus> {
    let remote = Remote::of(&project.dir).await.filter(is_github)?;
    let slug = &remote.path;
    let number_arg = number.to_string();
    let repo = format!("--repo={slug}");
    let fields = format!("--json={FIELDS}");
    let args = ["pr", "view", &number_arg, &repo, &fields];
    let text = match gh(PROGRAM, &project.dir, &args).await {
        Ok(text) => text,
        Err(error) => {
            tracing::debug!("pull request {number} not read: {error:#}");
            return None;
        }
    };
    one(&text, &remote, number)
}

/// The pull request of the current branch, as the forge has it; the newest when the
/// branch has several.
pub(super) async fn read_current(project: &Project) -> Result<PullStatus> {
    let fields = format!("--json={FIELDS}");
    let text = gh(PROGRAM, &project.dir, &["pr", "view", &fields]).await?;
    let raw: RawPull =
        serde_json::from_str(&text).context("gh gave a pull request it cannot read")?;
    Ok(raw.shown())
}

/// What `gh pr view --json` printed, when it is pull request `number` of `remote`.
fn one(text: &str, remote: &Remote, number: u64) -> Option<PullStatus> {
    let raw: RawPull = serde_json::from_str(text).ok()?;
    let pull = raw.shown();
    let page = format!("https://github.com/{}/pull/{number}", remote.path);
    (pull.number == number && pull.url.eq_ignore_ascii_case(&page)).then_some(pull)
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

/// Every check counted, the state the worst of them; and the first of them listed,
/// the ones that need a look first.
pub(super) fn roll_up(checks: &[RawCheck]) -> Checks {
    let mut counted = Checks::default();
    let checks = &checks[..checks.len().min(MAX_CHECKS)];
    let mut list: Vec<_> = checks.iter().map(RawCheck::shown).collect();
    // Stable, so checks of one stage keep the order GitHub gave.
    list.sort_by_key(|check| match check.stage {
        CheckStage::Failed => 0,
        CheckStage::Running => 1,
        CheckStage::Queued => 2,
        CheckStage::Passed => 3,
        CheckStage::Skipped => 4,
    });
    list.truncate(MAX_LISTED_CHECKS);
    counted.list = list;
    for check in checks {
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
    author: Option<RawAuthor>,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    changed_files: Option<u64>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    status_check_rollup: Vec<RawCheck>,
}

#[derive(Deserialize)]
struct RawAuthor {
    #[serde(default)]
    login: String,
}

impl RawPull {
    fn shown(self) -> PullStatus {
        let state = match self.state.as_str() {
            "MERGED" => PullState::Merged,
            "CLOSED" => PullState::Closed,
            _ if self.is_draft => PullState::Draft,
            _ => PullState::Open,
        };
        let url = link(&self.url);
        PullStatus {
            number: self.number,
            title: plain(&self.title, MAX_TITLE_CHARS).replace('\n', " "),
            state,
            branch: plain(&self.head_ref_name, 250),
            author: plain(&self.author.map(|a| a.login).unwrap_or_default(), 100)
                .replace('\n', " "),
            created: time(&self.created_at),
            updated: time(&self.updated_at),
            added: self.additions,
            removed: self.deletions,
            files: self.changed_files,
            // Without a query or fragment, and held to the same link rules.
            checks_url: link(&format!(
                "{}/checks",
                url.split(['?', '#']).next().unwrap_or("")
            )),
            url,
            checks: roll_up(&self.status_check_rollup),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(path: &str) -> Remote {
        Remote {
            host: "github.com".to_owned(),
            path: path.to_owned(),
        }
    }

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

    #[test]
    fn a_pull_carries_its_author_times_files_and_each_check() {
        let text = r#"{"number":13,"title":"Cards","state":"OPEN","headRefName":"x",
          "author":{"login":"ev\u202eil\u0007","is_bot":false},
          "createdAt":"2026-11-18T12:07:21Z","updatedAt":"not a time","changedFiles":7,
          "url":"https://github.com/o/r/pull/13",
          "statusCheckRollup":[
            {"__typename":"CheckRun","name":"fmt\u2066x","workflowName":"ci",
             "status":"COMPLETED","conclusion":"SUCCESS",
             "startedAt":"2026-11-18T12:14:01Z","completedAt":"2026-11-18T12:14:14Z",
             "detailsUrl":"https://github.com/o/r/actions/runs/1/job/2"},
            {"__typename":"CheckRun","name":"build","status":"QUEUED",
             "startedAt":"0001-01-01T00:00:00Z","detailsUrl":"javascript:alert(1)"},
            {"__typename":"StatusContext","context":"deploy","state":"FAILURE",
             "targetUrl":"https://ci.example.com/7"}]}"#;
        let (r, other) = (remote("o/r"), remote("o/other"));
        let pull = one(text, &r, 13).unwrap();
        assert_eq!(pull.author, "evil", "control and bidi characters are cut");
        assert_eq!(pull.created, Some(1_795_003_641));
        assert_eq!(pull.updated, None);
        assert_eq!(pull.files, Some(7));
        let list = &pull.checks.list;
        assert_eq!(list.len(), 3);
        assert_eq!(
            (list[0].name.as_str(), list[0].stage),
            ("deploy", CheckStage::Failed)
        );
        assert_eq!(list[0].url, "https://ci.example.com/7");
        assert_eq!(
            (list[1].stage, list[1].url.as_str()),
            (CheckStage::Queued, "")
        );
        assert_eq!(list[1].started, None, "the zero time is no time");
        let passed = &list[2];
        assert_eq!(
            (passed.name.as_str(), passed.workflow.as_str()),
            ("fmtx", "ci")
        );
        assert_eq!(
            passed.finished.zip(passed.started).map(|(f, s)| f - s),
            Some(13)
        );
        assert_eq!(pull.checks.state, Some(CheckState::Failing));
        assert_eq!(pull.checks_url, "https://github.com/o/r/pull/13/checks");
        let pull_at = |url: &str| {
            let text = format!(r#"[{{"number":3,"url":"{url}"}}]"#);
            parse(&text, 1).unwrap().remove(0).0
        };
        let queried = pull_at("https://github.com/o/r/pull/3?x=1#top");
        assert_eq!(queried.checks_url, "https://github.com/o/r/pull/3/checks");
        let long = format!("https://github.com/o/r/pull/3/{}", "a".repeat(465));
        assert!(long.len() <= 500 && pull_at(&long).checks_url.is_empty());
        assert!(pull_at("").checks_url.is_empty());

        // Another number, another repository, or a list in place of one: no card.
        assert!(one(text, &r, 12).is_none());
        assert!(one(text, &other, 13).is_none());
        assert!(one("[]", &r, 13).is_none());
        assert!(one("{", &r, 13).is_none());
    }

    #[test]
    fn the_listed_checks_need_a_look_first_and_are_capped() {
        let stages = [
            r#"{"name":"p","status":"COMPLETED","conclusion":"SUCCESS"}"#,
            r#"{"name":"s","status":"COMPLETED","conclusion":"SKIPPED"}"#,
            r#"{"name":"q","status":"QUEUED"}"#,
            r#"{"name":"r","status":"IN_PROGRESS"}"#,
            r#"{"name":"f","status":"COMPLETED","conclusion":"FAILURE"}"#,
        ];
        let checks: Vec<RawCheck> = (0..30)
            .flat_map(|_| stages.iter().map(|json| check(json)))
            .collect();
        let counted = roll_up(&checks);
        assert_eq!(counted.total, 150);
        assert_eq!(counted.list.len(), MAX_LISTED_CHECKS);
        let names: String = counted.list.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, format!("{}{}", "f".repeat(30), "r".repeat(20)));
        let few = roll_up(&checks[..5]);
        let names: Vec<_> = few.list.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["f", "r", "q", "p", "s"]);
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
        assert_eq!(reading.status.forge, Forge::None);
    }

    #[tokio::test]
    async fn a_signed_out_gh_is_a_sign_in_hint() {
        use std::{os::unix::fs::PermissionsExt, process::Command};
        // A repository with a github.com origin and a `gh` that exits 1 for everything.
        let dir = crate::testing::temp_dir("forge_sign_in");
        std::fs::create_dir_all(&dir).unwrap();
        for args in [
            &["init", "-q"][..],
            &["remote", "add", "origin", "git@github.com:o/r.git"][..],
        ] {
            let done = Command::new("git").args(args).current_dir(&dir).status();
            assert!(done.unwrap().success());
        }
        let gh = dir.join("gh");
        std::fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let reading = read_with(gh.to_str().unwrap(), &dir).await;
        let Forge::SignIn(sign_in) = &reading.status.forge else {
            panic!("{:?}", reading.status.forge)
        };
        assert_eq!(
            (sign_in.host.as_str(), sign_in.hint.as_str()),
            ("github.com", "gh auth login")
        );
        assert!(!reading.trouble, "signed out is an answer, not trouble");
    }
}
