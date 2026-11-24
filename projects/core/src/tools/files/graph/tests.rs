use std::{fs, sync::Arc};

use serde_json::{Value, json};

use super::{
    ProjectGraph,
    text::{MAX_BYTES, MAX_LINES},
};
use crate::{
    state::State,
    testing::{temp_dir, tool_context},
    tools::{
        ToolScope,
        tool::{Tool, ToolContext},
    },
};

async fn run(scope: &ToolScope, arguments: Value) -> anyhow::Result<String> {
    ProjectGraph.call(arguments, tool_context(scope)).await
}

fn scope(name: &str) -> ToolScope {
    let root = temp_dir(name);
    let (cwd, workspace) = (root.join("cwd"), root.join("workspace"));
    fs::create_dir_all(cwd.join("src/tools")).unwrap();
    fs::create_dir_all(cwd.join("target")).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    fs::write(cwd.join(".gitignore"), "target/\n").unwrap();
    fs::write(cwd.join("target/out.rlib"), "x").unwrap();
    fs::write(cwd.join("src/main.rs"), "fn main() {}").unwrap();
    fs::write(cwd.join("src/tools/read_file.rs"), "x").unwrap();
    fs::write(cwd.join("README.md"), "x").unwrap();
    ToolScope::new(&cwd, &workspace).unwrap()
}

#[tokio::test]
async fn maps_by_name_and_finds_by_part_or_glob() {
    let scope = scope("map_find");
    let map = run(&scope, json!({})).await.unwrap();
    assert_eq!(
        map,
        "./ (4 files)\n  src/\n    tools/\n      read_file.rs\n    main.rs\n  .gitignore\n  \
         README.md\n"
    );
    let found = run(&scope, json!({ "find": "READ" })).await.unwrap();
    assert_eq!(found, "README.md\nsrc/tools/read_file.rs\n");
    let found = run(&scope, json!({ "find": "*.rs", "path": "src/tools" }))
        .await
        .unwrap();
    assert_eq!(found, "src/tools/read_file.rs\n");
    let none = run(&scope, json!({ "find": "*.rlib" })).await.unwrap();
    assert!(none.starts_with("no path matches"), "{none}");
}

#[tokio::test]
async fn a_large_tree_folds_deep_folders_under_the_caps() {
    let scope = scope("map_fold");
    for crate_name in ["core", "tui"] {
        for module in 0..40 {
            let dir = scope.cwd().join(format!("projects/{crate_name}/m{module}"));
            fs::create_dir_all(&dir).unwrap();
            for file in 0..10 {
                fs::write(dir.join(format!("f{file}.rs")), "x").unwrap();
            }
        }
    }
    let map = run(&scope, json!({})).await.unwrap();
    assert!(map.lines().count() <= MAX_LINES + 2, "{map}");
    assert!(map.len() <= MAX_BYTES + 200, "{map}");
    assert!(map.contains("\n    core/\n      m0/ (10 files)\n"), "{map}");
    // A folded folder opens when asked for.
    let one = run(&scope, json!({ "path": "projects/core/m7" }))
        .await
        .unwrap();
    assert!(
        one.starts_with("projects/core/m7/ (10 files)\n  f0.rs\n"),
        "{one}"
    );
}

#[tokio::test]
async fn a_path_out_of_the_project_is_refused() {
    let scope = scope("map_out");
    assert!(
        run(&scope, json!({ "path": "../workspace" }))
            .await
            .is_err()
    );
    assert!(run(&scope, json!({ "path": "/etc" })).await.is_err());
    #[cfg(unix)]
    {
        let outside = temp_dir("map_out_target");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, scope.cwd().join("link")).unwrap();
        let error = run(&scope, json!({ "path": "link" })).await.unwrap_err();
        assert!(error.to_string().contains("inside the project"), "{error}");
        let map = run(&scope, json!({})).await.unwrap();
        assert!(!map.contains("secret"), "{map}");
    }
}

/// A saved project with a store, as a thread of it runs the tool.
async fn saved(name: &str, thread: &str) -> (ToolScope, ToolContext) {
    let scope = scope(name);
    let cwd = scope.cwd().canonicalize().unwrap();
    let scope = ToolScope::new(&cwd, scope.workspace()).unwrap();
    let state = State::in_memory().await.unwrap();
    state
        .create_project("p", "p", "", cwd.to_str().unwrap())
        .await
        .unwrap();
    let context = ToolContext {
        state: Some(Arc::new(state)),
        thread_id: thread.into(),
        ..tool_context(&scope)
    };
    (scope, context)
}

async fn call(context: &ToolContext, arguments: Value) -> anyhow::Result<String> {
    ProjectGraph.call(arguments, context.clone()).await
}

#[tokio::test]
async fn an_agent_records_a_file_and_the_map_and_about_show_it() {
    let (scope, context) = saved("graph_record", "").await;
    fs::write(
        scope.cwd().join("src/main.rs"),
        "fn main() {\n    run();\n}\n",
    )
    .unwrap();
    let map = call(&context, json!({})).await.unwrap();
    assert!(
        map.starts_with("./ (4 files, 4 lines)\n  src/ 3\n"),
        "{map}"
    );
    assert!(map.contains("    main.rs 3\n"), "{map}");

    let recorded = call(
        &context,
        json!({
            "action": "record",
            "path": "src/main.rs",
            "summary": "Starts the program.\nCalls run.",
            "nodes": [{"name": "main", "kind": "function", "line": 1, "end_line": 3}],
            "edges": [{"from": "main", "to": "run", "kind": "calls"}]
        }),
    )
    .await
    .unwrap();
    assert!(recorded.contains("summary, 1 nodes, 1 edges"), "{recorded}");
    let map = call(&context, json!({})).await.unwrap();
    assert!(
        map.contains("main.rs 3: Starts the program. Calls run.\n"),
        "{map}"
    );
    let about = call(&context, json!({"action": "about", "path": "src/main.rs"}))
        .await
        .unwrap();
    assert!(about.contains("function main 1-3"), "{about}");
    assert!(about.contains("main calls run"), "{about}");
    let found = call(&context, json!({"action": "find", "find": "MAIN"}))
        .await
        .unwrap();
    assert!(found.contains("function main (src/main.rs:1-3)"), "{found}");

    // The file changes: the map marks the summary until it is recorded again.
    fs::write(scope.cwd().join("src/main.rs"), "fn main() {}\n").unwrap();
    let path = scope.cwd().join("src/main.rs");
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(later)
        .unwrap();
    crate::project::graph::refresh_within(
        context.state.as_deref().unwrap(),
        scope.cwd(),
        &[],
        std::time::Duration::ZERO,
        std::time::Duration::from_secs(30),
    )
    .await
    .unwrap();
    let map = call(&context, json!({})).await.unwrap();
    assert!(
        map.contains("main.rs 1: Starts the program. Calls run. (stale)"),
        "{map}"
    );

    let fixed = call(
        &context,
        json!({"action": "fix", "path": "src/main.rs", "summary": "Does nothing yet."}),
    )
    .await
    .unwrap();
    assert!(fixed.starts_with("fixed the summary"), "{fixed}");
    let about = call(&context, json!({"action": "about", "path": "src/main.rs"}))
        .await
        .unwrap();
    assert!(
        about.contains("summary (by the user): Does nothing yet.\n"),
        "{about}"
    );
}

#[tokio::test]
async fn record_refuses_paths_out_of_the_project_and_files_of_secrets() {
    let (scope, context) = saved("graph_paths", "").await;
    fs::write(
        scope.cwd().join(".env"),
        "KEY=sk-123456789012345678901234\n",
    )
    .unwrap();
    let mut bad = vec![
        json!("../workspace/x"),
        json!("/etc/hosts"),
        json!("src/../../x"),
        json!(".env"),
        json!("src"),
        json!("none.rs"),
        // Ignored: the next refresh would drop its facts.
        json!("target/out.rlib"),
    ];
    #[cfg(unix)]
    {
        let outside = temp_dir("graph_paths_outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.rs"), "x").unwrap();
        std::os::unix::fs::symlink(outside.join("secret.rs"), scope.cwd().join("link.rs")).unwrap();
        bad.push(json!("link.rs"));
    }
    for path in bad {
        for action in ["record", "fix"] {
            let result = call(
                &context,
                json!({"action": action, "path": path, "summary": "x"}),
            )
            .await;
            assert!(result.is_err(), "{action} {path}");
        }
    }
    // Untrusted shapes: an unknown kind, an unknown field, too many nodes.
    for arguments in [
        json!({"action": "record", "path": "README.md",
               "nodes": [{"name": "a", "kind": "virus"}]}),
        json!({"action": "record", "path": "README.md", "summary": "x", "sql": "drop"}),
        json!({"action": "record", "path": "README.md",
               "nodes": (0..65).map(|n| json!({"name": format!("n{n}"), "kind": "other"}))
                   .collect::<Vec<_>>()}),
        json!({"action": "record", "path": "README.md", "summary": "s".repeat(401)}),
        json!({"action": "record", "path": "README.md"}),
    ] {
        assert!(
            call(&context, arguments.clone()).await.is_err(),
            "{arguments}"
        );
    }
    // A secret in a summary is not stored.
    call(
        &context,
        json!({"action": "record", "path": "README.md",
               "summary": "Uses token sk-abcdefghijklmnopqrstuvwxyz0123456789"}),
    )
    .await
    .unwrap();
    let about = call(&context, json!({"action": "about", "path": "README.md"}))
        .await
        .unwrap();
    assert!(!about.contains("sk-abcdefghijklmnopqrstuvwxyz"), "{about}");
}

#[tokio::test]
async fn a_member_records_only_the_files_of_its_task() {
    let (_scope, context) = saved("graph_member", "m1").await;
    let state = context.state.as_deref().unwrap();
    state.create_thread("lead", "test").await.unwrap();
    state
        .create_member_thread("m1", "lead", "general", "m1")
        .await
        .unwrap();
    let record = |path: &str| json!({"action": "record", "path": path, "summary": "x"});
    // No task held: nothing may be written.
    assert!(call(&context, record("src/main.rs")).await.is_err());
    let task = crate::state::NewTask {
        description: "t".into(),
        files: crate::state::TaskFiles::new(["src/"]).unwrap(),
        ..Default::default()
    };
    let added = state.add_plan_tasks("lead", &[task]).await.unwrap();
    let claim = crate::state::Claim {
        owner: "m1",
        at_once: 4,
        model: None,
        now: 1,
    };
    state
        .claim_task("lead", &added[0].task_id, claim)
        .await
        .unwrap();
    call(&context, record("src/main.rs")).await.unwrap();
    let error = call(&context, record("README.md")).await.unwrap_err();
    assert!(error.to_string().contains("task's files"), "{error}");
    // Reads are not cut.
    call(&context, json!({"action": "about", "path": "README.md"}))
        .await
        .unwrap();
}

#[tokio::test]
async fn answers_stay_under_the_caps() {
    let (scope, context) = saved("graph_caps", "").await;
    let nodes: Vec<Value> = (0..64)
        .map(|n| {
            json!({"name": format!("handler_{n}_{}", "x".repeat(100)), "kind": "function",
                        "summary": "y".repeat(200)})
        })
        .collect();
    for file in 0..4 {
        let path = format!("src/f{file}.rs");
        fs::write(scope.cwd().join(&path), "x\n").unwrap();
        call(
            &context,
            json!({"action": "record", "path": path, "nodes": nodes}),
        )
        .await
        .unwrap();
    }
    let found = call(&context, json!({"action": "find", "find": "handler"}))
        .await
        .unwrap();
    assert!(found.len() <= MAX_BYTES + 200, "{}", found.len());
    assert!(found.lines().count() <= MAX_LINES + 2);
    let about = call(
        &context,
        json!({"action": "about", "node": "handler_1_".to_owned() + &"x".repeat(100)}),
    )
    .await
    .unwrap();
    assert!(about.contains("more nodes named") || about.lines().count() <= MAX_LINES + 2);
    assert!(about.len() <= MAX_BYTES + 200);
}

#[tokio::test]
async fn only_record_and_fix_write() {
    for (action, reads) in [
        (None, true),
        (Some("map"), true),
        (Some("about"), true),
        (Some("find"), true),
        (Some("impact"), true),
        (Some("record"), false),
        (Some("fix"), false),
    ] {
        let arguments = match action {
            Some(action) => json!({"action": action}),
            None => json!({}),
        };
        assert_eq!(ProjectGraph.read_only(&arguments), reads, "{arguments}");
        assert_eq!(ProjectGraph.repeatable(&arguments), reads, "{arguments}");
    }
}

#[tokio::test]
async fn outside_a_saved_project_only_the_map_answers() {
    let scope = scope("graph_unsaved");
    let context = tool_context(&scope);
    assert!(run(&scope, json!({})).await.is_ok());
    let error = ProjectGraph
        .call(json!({"action": "find", "find": "x"}), context)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not a saved project"), "{error}");
}
