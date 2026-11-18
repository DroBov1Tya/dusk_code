//! The start of every request a thread sends: its system prompt, the project text and the
//! tool specifications. Providers cache a request by its prefix, so these tests hold that
//! it stays byte-identical while a thread runs.

use std::{path::Path, sync::Arc};

use serde_json::json;

use crate::{
    catalog::modes::{MAIN, Modes},
    config::Config,
    init::paths::Paths,
    providers::{ToolCall, ToolSpec},
    runtime::{Runtime, thread::project},
    tools::{Progress, ToolRegistry, ToolScope},
};

/// A chat's tools and the text before its messages, as the launcher builds them.
struct Thread {
    runtime: Runtime,
    paths: Paths,
    tools: ToolRegistry,
    scope: Arc<ToolScope>,
    prompt: String,
    project: String,
}

async fn thread(name: &str, cwd: &Path) -> Thread {
    let root = crate::testing::temp_dir(name);
    let paths = Paths::new(root.join("config"), root.join("work"));
    let runtime = Runtime::start(paths.clone(), Some(root.join("state.db")))
        .await
        .unwrap();
    let launcher = &runtime.launcher;
    let workspace = root.join("work/thread");
    std::fs::create_dir_all(&workspace).unwrap();
    let scope = Arc::new(launcher.scope(cwd, &workspace).unwrap());
    let prompt = Modes::new(&paths.modes_dir())
        .prompt(MAIN)
        .unwrap()
        .unwrap_or_default();
    Thread {
        tools: launcher.tools("t", None, 0),
        scope,
        prompt,
        project: project::context(cwd),
        runtime,
        paths,
    }
}

fn spec_bytes(spec: &ToolSpec) -> (usize, usize, usize) {
    let schema = serde_json::to_string(&spec.parameters).unwrap_or_default();
    (spec.name.len(), spec.description.len(), schema.len())
}

/// Prints the size of each part of a fresh chat's prefix in MAIN mode. Run with
/// `cargo test -p dusk_core prefix_sizes -- --ignored --nocapture`; `DUSK_PREFIX_CWD`
/// names the project, the repository by default.
#[tokio::test]
#[ignore = "a measurement, not a check"]
async fn prefix_sizes() {
    let cwd = std::env::var_os("DUSK_PREFIX_CWD").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        Into::into,
    );
    let cwd = cwd.canonicalize().unwrap();
    let thread = thread("prefix_sizes", &cwd).await;
    let config = thread.runtime.launcher.config.current();
    let specs = thread.tools.specs(&thread.scope, &config);
    println!("| part | bytes |\n|---|---|");
    println!("| system prompt (MAIN.md) | {} |", thread.prompt.len());
    println!("| project block | {} |", thread.project.len());
    let mut total = (0, 0, 0);
    let mut rows: Vec<_> = specs.iter().map(|spec| (spec, spec_bytes(spec))).collect();
    rows.sort_by_key(|(_, (n, d, s))| std::cmp::Reverse(n + d + s));
    for (spec, (name, description, schema)) in rows {
        total = (total.0 + name, total.1 + description, total.2 + schema);
        println!(
            "| tool {} | {} (description {description}, schema {schema}) |",
            spec.name,
            name + description + schema
        );
    }
    println!(
        "| {} tools | {} (names {}, descriptions {}, schemas {}) |",
        specs.len(),
        total.0 + total.1 + total.2,
        total.0,
        total.1,
        total.2
    );
    println!(
        "| prefix | {} |",
        thread.prompt.len() + thread.project.len() + total.0 + total.1 + total.2
    );
}

/// Both memories are on, as a user who uses them has it.
const MEMORIES: &str = "work_history:\n  enabled: true\n";

fn config(settings: &str) -> Arc<Config> {
    Arc::new(Config::parse(settings.as_bytes()).unwrap())
}

/// What every request of the thread starts with, in the order providers send it.
fn prefix(thread: &Thread, config: &Arc<Config>) -> String {
    let mut text = format!("{}\n{}\n", thread.prompt, thread.project);
    for spec in thread.tools.specs(&thread.scope, config) {
        text.push_str(&format!(
            "{}\n{}\n{}\n",
            spec.name, spec.description, spec.parameters
        ));
    }
    text
}

/// What used to move the prefix mid-thread: the model rewrites both memories through
/// its tools, and an agent added meanwhile is found by the catalog's periodic rescan.
/// The date is in the project text, which a thread reads once when it starts
/// (`Launcher::setup`), so a clock past midnight cannot move it.
async fn change_what_the_tools_describe(thread: &Thread, config: &Arc<Config>) {
    for (tool, memory) in [
        ("remember", "- prefers short answers"),
        ("remember_project", "- tests run with cargo test"),
    ] {
        let call = ToolCall {
            id: tool.into(),
            name: tool.into(),
            arguments: json!({ "memory": memory }),
        };
        let (events, _) = tokio::sync::mpsc::unbounded_channel();
        let progress = Progress {
            call_id: tool.into(),
            events,
        };
        let result = thread
            .tools
            .call(&call, &thread.scope, config, progress)
            .await;
        assert!(!result.is_error, "{}", result.content);
    }
    let agent = thread.paths.agents_dir().join("reviewer");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(
        agent.join("AGENT.md"),
        "---\ndescription: Reviews a change.\n---\nReview the change.\n",
    )
    .unwrap();
    thread.runtime.launcher.agents.scan().await.unwrap();
}

#[tokio::test]
async fn with_stable_prefix_the_start_of_a_request_stays_byte_identical() {
    let cwd = crate::testing::temp_dir("stable_prefix_cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let thread = thread("stable_prefix_on", &cwd).await;
    let config = config(&format!("limits:\n  stable_prefix: true\n{MEMORIES}"));
    let before = prefix(&thread, &config);
    change_what_the_tools_describe(&thread, &config).await;
    assert_eq!(prefix(&thread, &config), before);
    // After a compaction the tools show what changed.
    thread.tools.refresh_descriptions();
    let after = prefix(&thread, &config);
    for shown in [
        "- prefers short answers",
        "- tests run with cargo test",
        "- reviewer: Reviews a change.",
    ] {
        assert!(after.contains(shown), "{shown}");
    }
}

#[tokio::test]
async fn without_stable_prefix_each_request_shows_the_tools_as_they_are_now() {
    let cwd = crate::testing::temp_dir("live_prefix_cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let thread = thread("stable_prefix_off", &cwd).await;
    let config = config(MEMORIES);
    let before = prefix(&thread, &config);
    change_what_the_tools_describe(&thread, &config).await;
    let after = prefix(&thread, &config);
    assert_ne!(after, before);
    for shown in [
        "- prefers short answers",
        "- tests run with cargo test",
        "- reviewer: Reviews a change.",
    ] {
        assert!(after.contains(shown), "{shown}");
    }
    let now = thread.tools.descriptions(&thread.scope, &config);
    for spec in thread.tools.specs(&thread.scope, &config) {
        assert_eq!(
            Some(&spec.description),
            now.get(&spec.name),
            "{}",
            spec.name
        );
    }
}
