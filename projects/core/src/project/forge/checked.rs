//! A forge plugin's answers as the repository status shows them. The answers are checked
//! against the remote in `plugins::contract::forge`; here the pulls they keep get their
//! checks rolled up, and an error becomes the quiet note the user sees.

use dusk_protocol::{
    plugin::contract::{
        ContractError, ErrorKind,
        forge::{Check, Pull},
    },
    project::{self, CheckStage, CheckState, Checks, ForgeStatus, PullStatus},
};
use tracing::debug;

use super::Remote;
use crate::plugins::contract::forge::{self, MAX_CHECKS};

/// More checks than this on one pull request are counted but not listed.
pub(super) const MAX_LISTED: usize = 50;

/// The `ok` of `status` for `branch` as the status shows it, with how many of its pulls
/// are the branch's; `None` when the answer is `failed`.
pub fn status(
    ok: serde_json::Value,
    remote: &Remote,
    branch: Option<&str>,
    plugin: &str,
) -> Option<(ForgeStatus, usize)> {
    let answer = match forge::status(ok, remote, branch, plugin) {
        Ok(answer) => answer,
        Err(error) => {
            debug!("status of {plugin} refused: {}", error.message);
            return None;
        }
    };
    let mine = answer.branch_pulls.len();
    let pulls = answer
        .branch_pulls
        .into_iter()
        .chain(answer.open_pulls)
        .map(shown)
        .collect();
    let status = ForgeStatus {
        plugin: plugin.to_owned(),
        name: answer.name,
        repository: remote.path.clone(),
        pull_prefix: answer.pull_prefix,
        pulls,
        unavailable: None,
    };
    Some((status, mine))
}

/// The `ok` of `create_pull`: the pull made, in the shape of a pull; an answer in
/// another shape is `failed`.
pub fn made(ok: serde_json::Value, host: &str) -> Result<PullStatus, ContractError> {
    forge::made(ok, host).map(shown)
}

/// The `ok` of `pull`: the pull when it is `prefix` and its number (and `number`, when
/// asked); an answer in another shape is `failed`.
pub fn pull(
    ok: serde_json::Value,
    prefix: &str,
    host: &str,
    number: Option<u64>,
) -> Result<Option<PullStatus>, ContractError> {
    Ok(forge::pull(ok, prefix, host, number)?.map(shown))
}

/// What the status says for an error of the forge `name`; and whether it is trouble, to
/// ask again later while keeping the pulls known. `signed_out` is not handled here: it
/// is a sign-in row, not a note.
pub fn note(error: &ContractError, name: &str) -> (String, bool) {
    match error.kind {
        ErrorKind::NotFound => (format!("{name} does not show this repository"), false),
        ErrorKind::Unavailable => (format!("{name} did not answer"), true),
        _ => {
            debug!("{name} could not be read: {}", error.message);
            (format!("{name} could not be read"), false)
        }
    }
}

/// A checked pull as a row of the status.
fn shown(pull: Pull) -> PullStatus {
    PullStatus {
        number: pull.number,
        title: pull.title,
        state: pull.state.into(),
        branch: pull.branch,
        author: pull.author,
        created: pull.created,
        updated: pull.updated,
        added: pull.added,
        removed: pull.removed,
        files: pull.files,
        url: pull.url,
        checks_url: pull.checks_url,
        checks: roll_up(&pull.checks),
    }
}

/// Every check counted, the state the worst of them; and the first of them listed, the
/// ones that need a look first. Queued and running both count as running: a forge may
/// call a waiting check either.
pub fn roll_up(checks: &[Check]) -> Checks {
    let mut counted = Checks::default();
    let checks = &checks[..checks.len().min(MAX_CHECKS)];
    let mut list: Vec<project::Check> = checks.iter().map(listed).collect();
    // Stable, so checks of one stage keep the order the forge gave.
    list.sort_by_key(|check| match check.stage {
        CheckStage::Failed => 0,
        CheckStage::Running => 1,
        CheckStage::Queued => 2,
        CheckStage::Passed => 3,
        CheckStage::Skipped => 4,
    });
    list.truncate(MAX_LISTED);
    counted.list = list;
    for check in checks {
        counted.total += 1;
        match CheckStage::from(check.stage).state() {
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

/// A check already made plain, as listed.
fn listed(check: &Check) -> project::Check {
    project::Check {
        name: check.name.clone(),
        workflow: check.workflow.clone(),
        stage: check.stage.into(),
        url: check.url.clone(),
        started: check.started,
        finished: check.finished,
    }
}

#[cfg(test)]
mod tests {
    use dusk_protocol::plugin::contract::forge::Stage;
    use serde_json::{Value, json};

    use super::*;

    fn pull_answer(ok: &Value) -> Result<Option<PullStatus>, ContractError> {
        pull(
            ok.clone(),
            "https://github.com/o/r/pull/",
            "github.com",
            None,
        )
    }

    /// Spec 8, test 7.
    #[test]
    fn only_safe_links_and_own_pulls() {
        let remote = Remote {
            host: "github.com".into(),
            path: "o/r".into(),
        };
        let pull =
            |n: u64, url: &str| json!({"number": n, "state":"open", "branch":"x", "url": url});
        let answer = |prefix: &str, branch_pulls: Vec<Value>| json!({"name":"GitHub","pull_prefix":prefix,"branch_pulls":branch_pulls,"open_pulls":[]});
        for bad in [
            "http://github.com/o/r/pull/",
            "https://evil.com/o/r/pull/",
            "https://github.com/o/other/pull/",
            "https://github.com/o/r/pull",
        ] {
            assert!(
                status(answer(bad, vec![]), &remote, Some("x"), "github").is_none(),
                "{bad}"
            );
        }
        let p = "https://github.com/o/r/pull/";
        let (s, mine) = status(
            answer(
                p,
                vec![pull(1, &format!("{p}1")), pull(2, &format!("{p}3"))],
            ),
            &remote,
            Some("x"),
            "github",
        )
        .unwrap();
        assert_eq!(
            (s.pulls.len(), mine),
            (1, 1),
            "a url that is not prefix + number is dropped"
        );
        assert_eq!((s.name.as_str(), s.repository.as_str()), ("GitHub", "o/r"));

        // Links, text and bounds.
        let mut odd = pull(4, &format!("{p}4"));
        odd["checks_url"] = json!("https://evil.com/o/r/pull/4/checks");
        odd["title"] = json!(format!("\u{202e}{}", "t".repeat(10_000)));
        let checks: Vec<Value> = [
            "javascript:alert(1)",
            "http://ci.example.com/1",
            "https://a b",
        ]
        .iter()
        .map(|url| json!({"name":"c","stage":"failed","url":url}))
        .chain((0..600).map(|_| json!({"name":"q","stage":"queued"})))
        .collect();
        odd["checks"] = json!(checks);
        let mut fork = pull(5, &format!("{p}5"));
        fork["from_fork"] = json!(true);
        let mut many: Vec<Value> = (10..40).map(|n| pull(n, &format!("{p}{n}"))).collect();
        many.insert(0, fork);
        many.insert(0, odd);
        let (s, mine) = status(answer(p, many), &remote, Some("x"), "github").unwrap();
        assert_eq!(
            mine, 5,
            "30 branch pulls keep 5, and a fork's is not the branch's"
        );
        assert!(s.pulls.iter().all(|pull| pull.number != 5));
        let odd = &s.pulls[0];
        assert_eq!(odd.checks_url, "", "a checks page on another host");
        assert_eq!(odd.title, format!("{}...", "t".repeat(300)));
        assert_eq!((odd.checks.total, odd.checks.list.len()), (500, MAX_LISTED));
        assert!(
            odd.checks.list[..3]
                .iter()
                .all(|check| check.url.is_empty())
        );
        assert_eq!(odd.checks.state, Some(CheckState::Failing));

        // A result shaped for another operation is failed (spec test 6).
        assert!(pull_answer(&json!({"name":"GitHub","pull_prefix":p,"branch_pulls":[]})).is_err());
        assert_eq!(pull_answer(&json!({"pull": null})), Ok(None));
        assert!(status(json!({"pull": null}), &remote, Some("x"), "github").is_none());
    }

    #[test]
    fn queued_and_running_both_run_and_failed_comes_first() {
        let check = |name: &str, stage| Check {
            name: name.into(),
            workflow: String::new(),
            stage,
            url: String::new(),
            started: None,
            finished: None,
        };
        let stages = [
            ("p", Stage::Passed),
            ("s", Stage::Skipped),
            ("q", Stage::Queued),
            ("r", Stage::Running),
            ("f", Stage::Failed),
        ];
        let few: Vec<Check> = stages.iter().map(|(n, s)| check(n, *s)).collect();
        let counted = roll_up(&few);
        let names: Vec<_> = counted.list.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["f", "r", "q", "p", "s"]);
        assert_eq!((counted.passed, counted.running, counted.failed), (2, 2, 1));
        let waiting = roll_up(&[check("q", Stage::Queued), check("p", Stage::Passed)]);
        assert_eq!(waiting.state, Some(CheckState::Pending));
        assert_eq!(roll_up(&[]).state, None);
    }
}
