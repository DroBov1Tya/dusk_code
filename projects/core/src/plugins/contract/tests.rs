//! The boundaries of a contract call (spec 8): one project per call, the caps, and an
//! answer that is untrusted input.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use dusk_protocol::plugin::{
    ForgeProvides, Permissions, ProjectAccess, Provides, Tier,
    contract::{Call, ErrorKind},
};
use serde_json::json;

use super::{Contracts, Served, call::read_answer, forge};
use crate::{init::paths::Paths, platform, plugins::Isolation, project::forge::Remote};

/// A contract program in a file; what it does is its second argument.
const SCRIPT: &str = r#"#!/bin/sh
data="$1"
echo start >> "$data/starts"
case "$2" in
  seen)
    IFS= read -r line
    printf '%s\n' "$line" > "$data/seen"
    pwd >> "$data/seen"
    printf '%s\n' '{"type":"result","ok":{}}' ;;
  long)
    sleep 600 & echo $! > "$data/pid"
    head -c 5000000 /dev/zero | tr '\0' a; echo
    wait ;;
  silent)
    sleep 600 & echo $! > "$data/pid"
    wait ;;
  lingering)
    sleep 600 & echo $! > "$data/pid"
    exit 0 ;;
  busy)
    while [ ! -f "$data/go" ]; do sleep 0.05; done
    printf '%s\n' '{"type":"result","ok":{}}' ;;
  odd)
    printf '%s\n' '{"type":"result","error":{"kind":"rate_limited","message":"slow"}}' ;;
  unsupported)
    printf '%s\n' '{"type":"result","error":{"kind":"unsupported"}}' ;;
esac
"#;

/// A plugin and two projects under a root kept out of the temporary folders, which
/// stay writable for every plugin.
struct Setup {
    root: PathBuf,
    data: PathBuf,
    a: PathBuf,
    b: PathBuf,
    served: Served,
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn setup(mode: &str) -> Option<Setup> {
    if !platform::sandbox_available() {
        eprintln!("no sandbox on this system; skipped");
        return None;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/contract-tests")
        .join(format!("{mode}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths::new(root.join("config"), root.join("workspace"));
    let folder = paths.rules_root().join("plugins/p");
    let data = paths.config_root().join("plugin-data/p");
    let (a, b) = (root.join("a"), root.join("b"));
    for dir in [&folder, &data, &a, &b] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(folder.join("run.sh"), SCRIPT).unwrap();
    let permissions = Permissions {
        project: ProjectAccess::Read,
        ..Permissions::default()
    };
    let isolation = Isolation::new(&paths, &folder, &data, &permissions, Vec::new(), None).unwrap();
    let data = data.canonicalize().unwrap();
    let provides = Provides {
        command: Vec::new(),
        forge: Some(ForgeProvides {
            versions: vec![1, 7],
            hosts: vec!["github.com".into()],
        }),
        ..Provides::default()
    };
    let command = vec![
        "/bin/sh".to_owned(),
        folder.join("run.sh").display().to_string(),
        data.display().to_string(),
        mode.to_owned(),
    ];
    let served = Served::new("p".into(), Tier::Local, provides, command, isolation);
    Some(Setup {
        root,
        data,
        a,
        b,
        served,
    })
}

fn args() -> serde_json::Value {
    json!({"host": "github.com", "repository": "o/a", "branch": "x"})
}

fn starts(data: &Path) -> usize {
    std::fs::read_to_string(data.join("starts"))
        .unwrap_or_default()
        .lines()
        .count()
}

/// The process whose id is in `file` ends soon after.
fn gone(file: &Path) -> bool {
    let pid: i32 = std::fs::read_to_string(file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    for _ in 0..50 {
        if nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// Spec 8, test 1: the program is told one project, its canonical folder, and runs in it.
#[tokio::test]
async fn a_call_carries_one_project() {
    let Some(setup) = setup("seen") else {
        return;
    };
    let Setup {
        data, a, b, served, ..
    } = &setup;
    Contracts::default()
        .call(
            served,
            "forge",
            "status",
            a,
            std::slice::from_ref(b),
            args(),
        )
        .await
        .unwrap();
    let seen = std::fs::read_to_string(data.join("seen")).unwrap();
    let call: Call = serde_json::from_str(seen.lines().next().unwrap()).unwrap();
    assert_eq!(
        (call.contract.as_str(), call.op.as_str(), call.version),
        ("forge", "status", 1)
    );
    assert_eq!(Path::new(&call.folder), a.canonicalize().unwrap());
    assert_eq!(
        seen.lines().nth(1).unwrap(),
        a.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(!seen.contains(b.to_str().unwrap()) && !seen.contains("o/b"));
}

/// A line over `MAX_LINE` fails the call, and what the program started dies with it.
#[tokio::test]
async fn a_long_line_fails_and_kills() {
    let Some(setup) = setup("long") else {
        return;
    };
    let error = Contracts::default()
        .call(&setup.served, "forge", "status", &setup.a, &[], args())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Failed, "{}", error.message);
    assert!(gone(&setup.data.join("pid")));
}

/// A program that exits without a result while a child of it holds the pipe is `failed`
/// after a short grace, not at the 45 s limit, and the child is killed.
#[tokio::test]
async fn an_exit_without_a_result_does_not_wait_for_a_grandchild() {
    let Some(setup) = setup("lingering") else {
        return;
    };
    let start = std::time::Instant::now();
    let error = Contracts::default()
        .call(&setup.served, "forge", "status", &setup.a, &[], args())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Failed, "{}", error.message);
    assert!(start.elapsed() < Duration::from_secs(10));
    assert!(gone(&setup.data.join("pid")));
}

/// A line that would inflate when parsed fails.
#[tokio::test]
async fn a_large_many_item_result_fails() {
    let items = "{},".repeat(1_000_000);
    let text = format!("{{\"type\":\"result\",\"ok\":{{\"checks\":[{items}{{}}]}}}}\n");
    assert!(text.len() > 2_000_000 && text.len() < 4 * 1024 * 1024);
    let error = read_answer(&mut text.as_bytes(), "p", &mut Vec::new())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Failed);
    assert!(error.message.contains("too large"), "{}", error.message);
    let small = "{\"type\":\"result\",\"ok\":{\"checks\":[{},{}]}}\n";
    assert!(
        read_answer(&mut small.as_bytes(), "p", &mut Vec::new())
            .await
            .is_ok()
    );
}

/// No result in 45 s: `unavailable`, and the program's group is killed.
#[tokio::test(start_paused = true)]
async fn a_silent_program_is_unavailable_at_the_limit() {
    let Some(setup) = setup("silent") else {
        return;
    };
    let pid = setup.data.join("pid");
    let started = tokio::time::Instant::now();
    let contracts = Contracts::default();
    let call = contracts.call(&setup.served, "forge", "status", &setup.a, &[], args());
    // Busy until the program runs, so the paused clock does not jump before it does.
    let running = async {
        while !pid.exists() {
            tokio::task::yield_now().await;
        }
    };
    let (answer, ()) = tokio::join!(call, running);
    let error = answer.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unavailable, "{}", error.message);
    assert!(started.elapsed() >= Duration::from_secs(45));
    assert!(gone(&pid));
}

/// A fifth call at once waits for a slot before its program starts.
#[tokio::test]
async fn a_fifth_call_waits() {
    let Some(setup) = setup("busy") else {
        return;
    };
    let setup = Arc::new(setup);
    let contracts = Arc::new(Contracts::default());
    let calls: Vec<_> = (0..5)
        .map(|_| {
            let (setup, contracts) = (Arc::clone(&setup), Arc::clone(&contracts));
            tokio::spawn(async move {
                contracts
                    .call(&setup.served, "forge", "status", &setup.a, &[], args())
                    .await
            })
        })
        .collect();
    for _ in 0..400 {
        if starts(&setup.data) >= 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(starts(&setup.data), 4);
    std::fs::write(setup.data.join("go"), "").unwrap();
    for call in calls {
        call.await.unwrap().unwrap();
    }
    assert_eq!(starts(&setup.data), 5);
}

/// A kind this core does not know is `failed`; `unsupported` is `failed` and not asked
/// again in this run.
#[tokio::test]
async fn unknown_and_unsupported_kinds_are_failed() {
    let Some(odd) = setup("odd") else {
        return;
    };
    let contracts = Contracts::default();
    let error = contracts
        .call(&odd.served, "forge", "status", &odd.a, &[], args())
        .await
        .unwrap_err();
    assert_eq!(
        (error.kind, error.message.as_str()),
        (ErrorKind::Failed, "slow")
    );
    let Some(unsupported) = setup("unsupported") else {
        return;
    };
    for _ in 0..2 {
        let error = contracts
            .call(
                &unsupported.served,
                "forge",
                "pull",
                &unsupported.a,
                &[],
                args(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Failed);
    }
    assert_eq!(starts(&unsupported.data), 1);
}

/// Log lines are plain, capped and few; anything else before the result fails.
#[tokio::test]
async fn logs_are_capped_and_other_lines_fail() {
    let mut text = String::new();
    for _ in 0..30 {
        text.push_str(
            &json!({"type": "log", "text": format!("\u{202e}{}\n", "x".repeat(10_000))})
                .to_string(),
        );
        text.push('\n');
    }
    text.push_str("{\"type\":\"result\",\"ok\":{\"n\":1}}\n{\"type\":\"view\"}\n");
    let mut logs = Vec::new();
    let answer = read_answer(&mut text.as_bytes(), "p", &mut logs)
        .await
        .unwrap();
    assert_eq!(answer.ok, Some(json!({"n": 1})));
    assert_eq!(logs.len(), 20);
    assert!(
        logs.iter()
            .all(|log| log.chars().count() <= 503 && !log.contains(['\u{202e}', '\n']))
    );
    let other = "{\"type\":\"view\"}\n{\"type\":\"result\",\"ok\":{}}\n";
    let error = read_answer(&mut other.as_bytes(), "p", &mut Vec::new())
        .await
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Failed);
}

/// Spec 8, tests 6 and 7 (part): an answer in another operation's shape is `failed`; a
/// prefix off the remote's repository fails the whole answer; a pull whose page is not
/// the prefix and its number is dropped.
#[test]
fn answers_are_checked_against_the_remote() {
    let remote = Remote {
        host: "github.com".into(),
        path: "o/r".into(),
    };
    let prefix = "https://github.com/o/r/pull/";
    let pull = |number: u64, url: &str| json!({"number": number, "branch": "x", "url": url, "checks_url": "https://evil.com/c"});
    let status = |prefix: &str| json!({"name": "GitHub", "pull_prefix": prefix, "branch_pulls": [], "open_pulls": []});
    assert!(forge::pull(status(prefix), prefix, "github.com", None).is_err());
    let shaped_as_pull = json!({"pull": pull(1, &format!("{prefix}1"))});
    assert!(forge::status(shaped_as_pull, &remote, Some("x"), "p").is_err());
    for bad in [
        "https://evil.com/o/r/pull/",
        "http://github.com/o/r/pull/",
        "https://github.com/o/other/pull/",
        "https://github.com/o/r/pull",
        "https://github.com/o/r/../../evil/pull/",
    ] {
        assert!(
            forge::status(status(bad), &remote, Some("x"), "p").is_err(),
            "{bad}"
        );
    }
    let answer = json!({"name": "Git\nHub", "pull_prefix": prefix,
        "branch_pulls": [pull(1, &format!("{prefix}1")), pull(2, &format!("{prefix}3"))],
        "open_pulls": [pull(1, &format!("{prefix}1")), pull(4, "https://evil.com/o/r/pull/4")]});
    let answer = forge::status(answer, &remote, Some("x"), "p").unwrap();
    assert_eq!(answer.name, "p");
    let numbers: Vec<u64> = answer.branch_pulls.iter().map(|pull| pull.number).collect();
    assert_eq!(numbers, [1]);
    assert!(answer.open_pulls.is_empty());
    assert!(answer.branch_pulls[0].checks_url.is_empty());
    let one = json!({"pull": pull(13, &format!("{prefix}13"))});
    assert_eq!(
        forge::pull(one.clone(), prefix, "github.com", Some(13))
            .unwrap()
            .unwrap()
            .number,
        13
    );
    assert_eq!(
        forge::pull(one, prefix, "github.com", Some(12)).unwrap(),
        None
    );
    assert_eq!(
        forge::pull(json!({"pull": null}), prefix, "github.com", None).unwrap(),
        None
    );
}
