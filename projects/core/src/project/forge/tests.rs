//! The forge through a contract program: a fake plugin serving github.com answers what
//! its data folder holds for each operation.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use dusk_protocol::{
    plugin::{ForgeProvides, Permissions, ProjectAccess, Provides, Tier},
    project::{Forge, Project},
};
use serde_json::json;

use super::{Forges, Tracker, create, read_pull, read_status};
use crate::{
    init::paths::Paths,
    platform,
    plugins::{
        Isolation,
        contract::{Contracts, Served},
    },
};

/// Answers with the file named for the operation asked.
const SCRIPT: &str = r#"#!/bin/sh
data="$1"
touch "$data/ran"
IFS= read -r line
case "$line" in
  *'"op":"status"'*) cat "$data/status" ;;
  *'"op":"pull"'*) cat "$data/pull" ;;
  *'"op":"create_pull"'*) cat "$data/create_pull" ;;
esac
"#;

const PREFIX: &str = "https://github.com/o/a/pull/";

struct Setup {
    root: PathBuf,
    data: PathBuf,
    project: Project,
    forges: Forges,
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Setup {
    /// The plugin answers `op` with this `result` line.
    fn answer(&self, op: &str, result: serde_json::Value) {
        let mut line = json!({"type": "result"});
        line.as_object_mut()
            .unwrap()
            .extend(result.as_object().unwrap().clone());
        std::fs::write(self.data.join(op), format!("{line}\n")).unwrap();
    }
}

fn setup(name: &str) -> Option<Setup> {
    if !platform::sandbox_available() {
        eprintln!("no sandbox on this system; skipped");
        return None;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/forge-contract-tests")
        .join(format!("{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths::new(root.join("config"), root.join("workspace"));
    let folder = paths.rules_root().join("plugins/p");
    let data = paths.config_root().join("plugin-data/p");
    let (a, b) = (root.join("a"), root.join("b"));
    for dir in [&folder, &data, &a, &b] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for args in [
        &["init", "-q"][..],
        &["symbolic-ref", "HEAD", "refs/heads/x"][..],
        &["remote", "add", "origin", "https://github.com/o/a.git"][..],
    ] {
        let done = Command::new("git").args(args).current_dir(&a).status();
        assert!(done.unwrap().success());
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
            versions: vec![1],
            hosts: vec!["github.com".into()],
        }),
        ..Provides::default()
    };
    let command = vec![
        "/bin/sh".to_owned(),
        folder.join("run.sh").display().to_string(),
        data.display().to_string(),
    ];
    let contracts = Arc::new(Contracts::default());
    contracts.set(vec![Served::new(
        "github".into(),
        Tier::Local,
        provides,
        command,
        isolation,
    )]);
    let project = Project {
        id: "a".into(),
        name: "a".into(),
        description: String::new(),
        dir: a.canonicalize().unwrap(),
        created_at: 0,
    };
    let forges = Forges {
        contracts,
        on: true,
        chosen: Default::default(),
        others: vec![b],
    };
    Some(Setup {
        root,
        data,
        project,
        forges,
    })
}

fn pull(number: u64, url: &str) -> serde_json::Value {
    json!({"number": number, "title": "t", "state": "open", "branch": "x", "url": url,
           "checks": [{"name": "build", "stage": "queued"}]})
}

fn status_answer() -> serde_json::Value {
    json!({"ok": {"name": "GitHub", "pull_prefix": PREFIX,
        "branch_pulls": [pull(1, &format!("{PREFIX}1"))],
        "open_pulls": [pull(2, &format!("{PREFIX}2")),
                       pull(3, "https://github.com/someone/else/pull/3")]}})
}

/// A served host is read through the plugin; a pull of another repository is dropped,
/// and signed out is a sign-in row with the plugin's hint, not trouble.
#[tokio::test]
async fn a_served_host_reads_through_the_plugin() {
    let Some(setup) = setup("status") else {
        return;
    };
    setup.answer("status", status_answer());
    let reading = read_status(&setup.project, &setup.forges).await;
    let Forge::Served(served) = &reading.status.forge else {
        panic!("{:?}", reading.status.forge);
    };
    let numbers: Vec<u64> = served.pulls.iter().map(|pull| pull.number).collect();
    assert_eq!(numbers, [1, 2], "the pull of another repository is dropped");
    assert_eq!(
        (served.plugin.as_str(), served.name.as_str()),
        ("github", "GitHub")
    );
    assert_eq!(served.pull_prefix, PREFIX);
    assert_eq!(
        served.pulls[0].checks.running, 1,
        "queued counts as running"
    );
    assert_eq!((reading.mine, reading.trouble), (1, false));

    let hint = "gh auth login --insecure-storage";
    setup.answer(
        "status",
        json!({"error": {"kind": "signed_out", "message": "no token", "hint": hint}}),
    );
    let reading = read_status(&setup.project, &setup.forges).await;
    let Forge::SignIn(sign_in) = &reading.status.forge else {
        panic!("{:?}", reading.status.forge);
    };
    assert_eq!(
        (
            sign_in.host.as_str(),
            sign_in.plugin.as_str(),
            sign_in.hint.as_str()
        ),
        ("github.com", "github", hint)
    );
    assert!(!reading.trouble);
}

/// With the switch off the plugin is never started: github.com goes to `gh`.
#[tokio::test]
async fn the_switch_off_never_starts_the_plugin() {
    let Some(mut setup) = setup("off") else {
        return;
    };
    setup.answer("status", status_answer());
    setup.forges.on = false;
    let reading =
        super::status::read_with("/nonexistent/gh", &setup.project.dir, &setup.forges).await;
    let Forge::SignIn(sign_in) = &reading.status.forge else {
        panic!("{:?}", reading.status.forge);
    };
    assert_eq!(sign_in.hint, "gh auth login", "the built-in path answered");
    assert!(!setup.data.join("ran").exists(), "the plugin was started");
    setup.forges.on = true;
    read_status(&setup.project, &setup.forges).await;
    assert!(setup.data.join("ran").exists());
}

/// A plugin that does not answer is trouble: the tracker keeps the pulls, the name and
/// the prefix it had.
#[tokio::test]
async fn an_unavailable_plugin_keeps_the_old_pulls() {
    let Some(setup) = setup("unavailable") else {
        return;
    };
    let tracker = Tracker::default();
    tracker.watch(1, "a");
    setup.answer("status", status_answer());
    tracker.finish("a", read_status(&setup.project, &setup.forges).await);

    setup.answer(
        "status",
        json!({"error": {"kind": "unavailable", "message": "offline"}}),
    );
    let reading = read_status(&setup.project, &setup.forges).await;
    assert!(reading.trouble);
    assert_eq!(
        reading
            .status
            .forge
            .served()
            .unwrap()
            .unavailable
            .as_deref(),
        Some("github did not answer")
    );
    let (status, _) = tracker.finish("a", reading);
    let served = status.forge.served().unwrap();
    assert_eq!(served.pulls.len(), 2);
    assert_eq!(
        (served.name.as_str(), served.pull_prefix.as_str()),
        ("GitHub", PREFIX)
    );
    assert_eq!(tracker.prefix("a").as_deref(), Some(PREFIX));
}

/// A refusal of `create_pull` is shown in the forge's words, and the pull it made is taken
/// on its shape alone; a pull lookup keeps only the pull at the prefix and number asked.
#[tokio::test]
async fn create_refused_and_lookups_by_the_prefix() {
    let Some(setup) = setup("create") else {
        return;
    };
    let refused = "a pull request for branch \"x\" already exists";
    setup.answer(
        "create_pull",
        json!({"error": {"kind": "refused", "message": refused}}),
    );
    let error = create(&setup.project, "Fix", "", None, &setup.forges)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), refused);

    setup.answer(
        "create_pull",
        json!({"ok": {"pull": {"number": 4, "url": format!("{PREFIX}4")}}}),
    );
    let made = create(&setup.project, "Fix", "body", None, &setup.forges)
        .await
        .unwrap();
    assert_eq!(
        (made.number, made.url.as_str()),
        (4, format!("{PREFIX}4").as_str())
    );
    for amiss in [
        json!({"pull": null}),
        json!({"pull": {"number": 4}}),
        json!({}),
    ] {
        setup.answer("create_pull", json!({ "ok": amiss }));
        let error = create(&setup.project, "Fix", "", None, &setup.forges)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("it may have been made"),
            "{error}"
        );
    }
    setup.answer(
        "pull",
        json!({"ok": {"pull": pull(7, &format!("{PREFIX}7"))}}),
    );
    let found = read_pull(&setup.project, 7, &setup.forges, PREFIX).await;
    assert_eq!(found.map(|pull| pull.number), Some(7));
    setup.answer(
        "pull",
        json!({"ok": {"pull": pull(7, "https://github.com/someone/else/pull/7")}}),
    );
    assert!(
        read_pull(&setup.project, 7, &setup.forges, PREFIX)
            .await
            .is_none()
    );
}
