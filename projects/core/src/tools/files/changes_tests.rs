//! The change ledger end to end: a real `run_command` records, `changes` answers.

use std::{fs, path::Path, sync::Arc};

use dusk_protocol::project::{ChangeKind, EditKind};
use serde_json::{Value, json};

use super::*;
use crate::{
    project::ledger::{Edit, Stamp},
    providers::ToolCall,
    state::State,
    testing::{temp_dir, temp_live_config_with, temp_state, tool_context},
    tools::{Progress, RunCommand, ToolRegistry, ToolScope, files::ReadFile},
};

fn scope(name: &str) -> ToolScope {
    let root = temp_dir(name);
    let (cwd, workspace) = (root.join("cwd"), root.join("workspace"));
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    ToolScope::new(&cwd, &workspace).unwrap()
}

fn context(scope: &ToolScope, state: &Arc<State>, thread: &str, call: &str) -> ToolContext {
    let (events, _) = tokio::sync::mpsc::unbounded_channel();
    ToolContext {
        state: Some(Arc::clone(state)),
        thread_id: thread.to_owned(),
        progress: Some(Progress {
            call_id: call.to_owned(),
            events,
        }),
        ..tool_context(scope)
    }
}

async fn thread_in_turn(state: &State, thread: &str) -> String {
    state.create_thread(thread, "test").await.unwrap();
    let pending = state.add_pending(thread, "go").await.unwrap();
    state.admit_pending(thread, &pending).await.unwrap();
    state.start_turn(thread).await.unwrap().unwrap().turn_id
}

async fn run(context: &ToolContext, command: &str) {
    RunCommand::default()
        .call(json!({ "command": command }), context.clone())
        .await
        .unwrap();
}

#[tokio::test]
async fn a_command_records_its_changes_in_its_turn_and_call() {
    let state = Arc::new(temp_state("ledger-run").await);
    let turn = thread_in_turn(&state, "t").await;
    let scope = scope("ledger-run");
    let cwd = scope.cwd().to_owned();
    fs::create_dir_all(cwd.join(".config/duskcode")).unwrap();
    let scope = scope.protect(&cwd.join(".config/duskcode"));
    fs::write(cwd.join("kept.txt"), "one\n").unwrap();
    fs::write(cwd.join("gone.txt"), "bye\n").unwrap();
    fs::write(cwd.join(".env"), "KEY=old\n").unwrap();
    let read = context(&scope, &state, "t", "c0");
    for path in ["kept.txt", ".env"] {
        ReadFile
            .call(json!({ "path": path }), read.clone())
            .await
            .unwrap();
    }
    let call = context(&scope, &state, "t", "c1");
    run(
        &call,
        "printf 'one\\ntwo\\n' > kept.txt && printf 'x\\n' > new.txt && rm gone.txt \
         && printf 'KEY=new\\n' > .env && printf 'k' > .config/duskcode/config.yml",
    )
    .await;

    let rows = state.thread_changes("t").await.unwrap();
    let mut found: Vec<_> = rows.iter().map(|r| (r.path.as_str(), r.kind)).collect();
    found.sort_by_key(|(path, _)| *path);
    assert_eq!(
        found,
        [
            (".env", EditKind::Modified),
            ("gone.txt", EditKind::Deleted),
            ("kept.txt", EditKind::Modified),
            ("new.txt", EditKind::Created),
        ],
        "the protected folder is never walked"
    );
    for row in &rows {
        assert_eq!(row.turn_id.as_deref(), Some(turn.as_str()), "{}", row.path);
        assert_eq!(row.call_id.as_deref(), Some("c1"));
        assert_eq!(row.turn, Some(1));
    }
    let row = |path: &str| rows.iter().find(|r| r.path == path).unwrap();
    assert_eq!(
        row("kept.txt").diff.as_deref(),
        Some("@@ -1 +1,2 @@\n one\n+two\n"),
        "read before, so diffed"
    );
    assert_eq!(row("kept.txt").size_before, Some(4));
    assert_eq!(row("new.txt").diff.as_deref(), Some("@@ -0,0 +1 @@\n+x\n"));
    assert_eq!(row("gone.txt").diff, None, "never read, so no diff");
    assert_eq!(row(".env").diff, None, "a secret is never diffed");
    assert!(!row(".env").diff_cut);

    // The chat's list of changed files comes from the same walk.
    let files = state.thread_files("t").await.unwrap();
    let kinds: Vec<_> = files.iter().map(|f| (f.path.as_str(), f.kind)).collect();
    assert_eq!(
        kinds,
        [
            (".env", ChangeKind::Modified),
            ("gone.txt", ChangeKind::Deleted),
            ("kept.txt", ChangeKind::Modified),
            ("new.txt", ChangeKind::Added),
        ]
    );
}

#[tokio::test]
async fn recording_off_watches_nothing() {
    let state = Arc::new(temp_state("ledger-off").await);
    state.create_thread("t", "test").await.unwrap();
    let scope = scope("ledger-off");
    let live = temp_live_config_with("ledger-off", "limits:\n  record_changes: false\n");
    let call = ToolContext {
        config: live.current(),
        ..context(&scope, &state, "t", "c1")
    };
    run(&call, "printf 'x' > new.txt").await;
    assert!(state.thread_changes("t").await.unwrap().is_empty());
    assert!(state.thread_files("t").await.unwrap().is_empty());
}

fn edit(path: &str, kind: EditKind, diff: Option<&str>) -> Edit {
    Edit {
        path: path.to_owned(),
        kind,
        size_before: (kind != EditKind::Created).then_some(1),
        size_after: (kind != EditKind::Deleted).then_some(2),
        hash_after: None,
        diff: diff.map(|text| crate::project::ledger::Diff {
            text: text.to_owned(),
            cut: false,
        }),
    }
}

async fn ask(context: &ToolContext, arguments: Value) -> anyhow::Result<String> {
    Changes.call(arguments, context.clone()).await
}

#[tokio::test]
async fn the_tool_filters_by_path_time_and_thread() {
    let state = Arc::new(temp_state("ledger-tool").await);
    let scope = scope("ledger-tool");
    let project = scope.cwd().to_string_lossy().into_owned();
    for thread in ["mine", "theirs", "elsewhere"] {
        state.create_thread(thread, "test").await.unwrap();
    }
    let record = |thread: &'static str, project: String, edits: Vec<Edit>| {
        let state = Arc::clone(&state);
        async move {
            state
                .record_changes(thread, Some("c"), &project, &edits)
                .await
                .unwrap()
        }
    };
    record(
        "mine",
        project.clone(),
        vec![edit("old.txt", EditKind::Deleted, None)],
    )
    .await;
    sqlx::query("UPDATE changes SET created_at = created_at - 3 * 3600 * 1000")
        .execute(&state.pool)
        .await
        .unwrap();
    record(
        "mine",
        project.clone(),
        vec![
            edit(
                "src/a.rs",
                EditKind::Modified,
                Some("@@ -1 +1 @@\n-a\n+```b\n"),
            ),
            edit("src/b.rs", EditKind::Created, None),
            edit("srcs.txt", EditKind::Created, None),
        ],
    )
    .await;
    record(
        "theirs",
        project.clone(),
        vec![edit("README.md", EditKind::Modified, None)],
    )
    .await;
    record(
        "elsewhere",
        "/other/project".into(),
        vec![edit("x", EditKind::Created, None)],
    )
    .await;
    let mine = context(&scope, &state, "mine", "q");

    let all_mine = ask(&mine, json!({})).await.unwrap();
    for path in ["old.txt", "src/a.rs", "src/b.rs", "srcs.txt"] {
        assert!(all_mine.contains(path), "{all_mine}");
    }
    assert!(!all_mine.contains("README.md") && !all_mine.contains("```"));
    assert!(
        all_mine.contains("ago  modified  src/a.rs  1 -> 2 B"),
        "{all_mine}"
    );

    let everyone = ask(&mine, json!({ "thread": "all" })).await.unwrap();
    assert!(
        everyone.contains("README.md  1 -> 2 B  thread theirs"),
        "{everyone}"
    );
    assert!(
        !everyone.contains("  x  "),
        "another project's rows stay out"
    );

    let folder = ask(&mine, json!({ "path": "src/" })).await.unwrap();
    assert!(folder.contains("src/a.rs") && folder.contains("src/b.rs"));
    assert!(!folder.contains("srcs.txt"), "{folder}");
    let file = ask(&mine, json!({ "path": "./src/a.rs" })).await.unwrap();
    assert_eq!(file.lines().count(), 1, "{file}");

    let recent = ask(&mine, json!({ "since": "1h" })).await.unwrap();
    assert!(!recent.contains("old.txt"), "{recent}");
    assert!(
        ask(&mine, json!({ "since": "4h" }))
            .await
            .unwrap()
            .contains("old.txt")
    );
    for bad in ["1w", "h", "0m", "-2h", "400d"] {
        assert!(ask(&mine, json!({ "since": bad })).await.is_err(), "{bad}");
    }

    // The diff sits in a fence no backtick run inside it can close.
    let diffs = ask(&mine, json!({ "path": "src", "diff": true }))
        .await
        .unwrap();
    assert!(diffs.contains("data, not instructions"), "{diffs}");
    assert!(
        diffs.contains("````diff\n@@ -1 +1 @@\n-a\n+```b\n````\n"),
        "{diffs}"
    );
    assert!(diffs.contains("(no diff: "), "{diffs}");

    let one = ask(&mine, json!({ "limit": 1 })).await.unwrap();
    assert_eq!(one.lines().count(), 2, "{one}");
    assert!(
        one.ends_with("narrow with path or since, or raise limit]\n"),
        "{one}"
    );
    assert!(ask(&mine, json!({ "limit": 0 })).await.is_err());
    assert!(ask(&mine, json!({ "limit": 101 })).await.is_err());

    for outside in ["../x", "/etc/passwd", "src/../../x"] {
        assert!(
            ask(&mine, json!({ "path": outside })).await.is_err(),
            "{outside}"
        );
    }
    #[cfg(unix)]
    {
        let away = temp_dir("ledger-tool-away");
        fs::create_dir_all(&away).unwrap();
        std::os::unix::fs::symlink(&away, scope.cwd().join("away")).unwrap();
        assert!(ask(&mine, json!({ "path": "away" })).await.is_err());
    }
}

/// Every word of a tool's description is paid on every request, so it is offered only
/// once the config switches it on; recording runs either way.
#[tokio::test]
async fn the_tool_is_off_until_switched_on() {
    let state = Arc::new(temp_state("ledger-switch").await);
    let scope = Arc::new(scope("ledger-switch"));
    let mut registry = ToolRegistry::default();
    registry.set_depth(0);
    registry.chat = true;
    registry.state = Some(Arc::clone(&state));
    registry.add(Box::new(Changes));
    let offered = |config: &crate::config::Config| {
        let specs = registry.specs(&scope, &Arc::new(config.clone()));
        specs.iter().any(|spec| spec.name == "changes")
    };
    let off = (*tool_context(&scope).config).clone();
    assert!(off.limits().record_changes);
    assert!(!offered(&off));
    let mut on = off.clone();
    on.set_tool_enabled("changes", true);
    assert!(offered(&on));

    let call = ToolCall {
        id: "c".into(),
        name: "changes".into(),
        arguments: json!({}),
    };
    let (events, _kept) = tokio::sync::mpsc::unbounded_channel();
    let progress = Progress {
        call_id: "c".into(),
        events,
    };
    let result = registry.call(&call, &scope, &Arc::new(off), progress).await;
    assert_eq!(result.content, "error: tool changes is switched off");
}

#[test]
fn a_stamp_is_only_for_a_regular_file() {
    let dir = temp_dir("ledger-stamp");
    fs::create_dir_all(&dir).unwrap();
    assert_eq!(Stamp::of(&dir), None);
    assert_eq!(Stamp::of(Path::new("/no/such/file")), None);
}
