//! A forge answer checked against the remote it is about (spec 4.4): a trusted pull
//! prefix on the remote's own repository, only pulls whose page is that prefix and
//! their number, https links, plain capped text and bounded lists. An answer in another
//! operation's shape is `failed`, never read as an empty one.

use dusk_protocol::plugin::contract::{
    ContractError,
    forge::{Check, Pull, PullAnswer, StatusAnswer},
};

use super::{failed, untrusted};
use crate::project::forge::Remote;

pub const MAX_BRANCH_PULLS: usize = 5;
pub const MAX_OPEN_PULLS: usize = 10;
pub const MAX_CHECKS: usize = 500;
const MAX_NAME: usize = 40;
const MAX_PREFIX: usize = 300;
const MAX_TITLE: usize = 300;
const MAX_AUTHOR: usize = 100;
const MAX_BRANCH: usize = 250;
const MAX_WORKFLOW: usize = 200;

/// The `ok` of `status` for `branch` of `remote`, checked; `plugin` names the forge when
/// the answer's name is not a short plain line.
pub fn status(
    ok: serde_json::Value,
    remote: &Remote,
    branch: Option<&str>,
    plugin: &str,
) -> Result<StatusAnswer, ContractError> {
    let has = |key: &str| ok.get(key).is_some();
    if !has("pull_prefix") {
        return Err(failed("the status answer has no pull_prefix".into()));
    }
    let answer: StatusAnswer =
        serde_json::from_value(ok).map_err(|error| shape_error("status", &error))?;
    if !is_prefix(&answer.pull_prefix, remote) {
        return Err(failed(format!(
            "the pull prefix is not on {}/{}",
            remote.host, remote.path
        )));
    }
    let prefix = answer.pull_prefix;
    let name = if untrusted::line(&answer.name, MAX_NAME) == answer.name
        && !answer.name.trim().is_empty()
    {
        answer.name
    } else {
        plugin.to_owned()
    };
    // A fork's branch of the same name is someone else's.
    let branch_pulls: Vec<Pull> = answer
        .branch_pulls
        .into_iter()
        .filter(|pull| !pull.from_fork && Some(pull.branch.as_str()) == branch)
        .filter_map(|pull| checked(pull, &prefix, &remote.host))
        .take(MAX_BRANCH_PULLS)
        .collect();
    let mut open_pulls: Vec<Pull> = Vec::new();
    for pull in answer.open_pulls {
        if open_pulls.len() == MAX_OPEN_PULLS {
            break;
        }
        let known = |number: u64| {
            branch_pulls
                .iter()
                .chain(&open_pulls)
                .any(|pull| pull.number == number)
        };
        if !known(pull.number)
            && let Some(pull) = checked(pull, &prefix, &remote.host)
        {
            open_pulls.push(pull);
        }
    }
    Ok(StatusAnswer {
        name,
        pull_prefix: prefix,
        branch_pulls,
        open_pulls,
    })
}

/// The `ok` of `create_pull`: the pull made, in the shape of a pull, its text plain and
/// capped and its links safe and on the host; not whose page it is.
pub fn made(ok: serde_json::Value, host: &str) -> Result<Pull, ContractError> {
    let answer: PullAnswer =
        serde_json::from_value(ok).map_err(|error| shape_error("create_pull", &error))?;
    let Some(pull) = answer.pull.filter(|pull| pull.number > 0) else {
        return Err(failed("the create_pull answer has no pull".into()));
    };
    let page = untrusted::link(&pull.url);
    if !on_host(&page, host) {
        return Err(failed("the pull made has no safe page on its host".into()));
    }
    Ok(cleaned(pull, page, host))
}

/// The `ok` of `pull`, checked against the prefix of the last status; `None` when there
/// is none or it is not the remote's (or not `number`, when asked).
pub fn pull(
    ok: serde_json::Value,
    prefix: &str,
    host: &str,
    number: Option<u64>,
) -> Result<Option<Pull>, ContractError> {
    if ok.get("pull").is_none() {
        return Err(failed("the pull answer has no pull".into()));
    }
    let answer: PullAnswer =
        serde_json::from_value(ok).map_err(|error| shape_error("pull", &error))?;
    Ok(answer
        .pull
        .filter(|pull| number.is_none_or(|number| pull.number == number))
        .and_then(|pull| checked(pull, prefix, host)))
}

/// `https://{host}/{path}/...`/, of the safe link characters, with no query, fragment,
/// escape or dot segment that could lead off the repository.
fn is_prefix(prefix: &str, remote: &Remote) -> bool {
    let own = format!("https://{}/{}/", remote.host, remote.path);
    let rest = prefix.get(own.len()..).unwrap_or_default();
    prefix.len() <= MAX_PREFIX
        && prefix.len() >= own.len()
        && prefix.is_char_boundary(own.len())
        && prefix[..own.len()].eq_ignore_ascii_case(&own)
        && prefix.ends_with('/')
        && untrusted::link(prefix) == prefix
        && !rest.contains(['?', '#', '%'])
        && !rest
            .split('/')
            .any(|segment| segment == "." || segment == "..")
}

/// A pull whose page is `prefix` and its number, its text plain and capped; `None`
/// otherwise.
fn checked(pull: Pull, prefix: &str, host: &str) -> Option<Pull> {
    let page = format!("{prefix}{}", pull.number);
    if pull.number == 0 || !pull.url.eq_ignore_ascii_case(&page) {
        return None;
    }
    Some(cleaned(pull, page, host))
}

/// `pull` with its text plain and capped, its page `page`, and links only on `host`.
fn cleaned(pull: Pull, page: String, host: &str) -> Pull {
    let checks_url = untrusted::link(&pull.checks_url);
    let checks_url = match on_host(&checks_url, host) {
        true => checks_url,
        false => String::new(),
    };
    Pull {
        number: pull.number,
        title: untrusted::line(&pull.title, MAX_TITLE),
        state: pull.state,
        branch: untrusted::line(&pull.branch, MAX_BRANCH),
        from_fork: pull.from_fork,
        author: untrusted::line(&pull.author, MAX_AUTHOR),
        created: seconds(pull.created),
        updated: seconds(pull.updated),
        added: pull.added,
        removed: pull.removed,
        files: pull.files,
        url: page,
        checks_url,
        checks: pull
            .checks
            .into_iter()
            .take(MAX_CHECKS)
            .map(|check| Check {
                name: untrusted::line(&check.name, MAX_TITLE),
                workflow: untrusted::line(&check.workflow, MAX_WORKFLOW),
                // Queued and running both stand: a plugin maps pending either way.
                stage: check.stage,
                url: untrusted::link(&check.url),
                started: seconds(check.started),
                finished: seconds(check.finished),
            })
            .collect(),
    }
}

/// `link` is a page on `host` (ASCII case-insensitive).
fn on_host(link: &str, host: &str) -> bool {
    let start = format!("https://{host}/");
    link.get(..start.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(&start))
}

/// Why an answer is not of its operation's shape. serde quotes the program's words (an
/// unknown variant), so they are made plain before they reach the debug log.
fn shape_error(op: &str, error: &serde_json::Error) -> ContractError {
    failed(untrusted::line(
        &format!("the {op} answer is not one: {error}"),
        300,
    ))
}

fn seconds(time: Option<i64>) -> Option<i64> {
    time.filter(|seconds| *seconds > 0)
}
