//! Steps that bring nothing new get a note, then a re-plan and a stall mark; work that
//! moves on, research included, gets none.

use dusk_protocol::thread::RepeatRung;
use serde_json::{Value, json};

use super::{Facts, Repeats};
use crate::{config::Limits, providers::ToolCall, tools::ToolResult};

fn call(name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        id: "call".into(),
        name: name.into(),
        arguments,
    }
}

fn command(text: &str) -> ToolCall {
    call("run_command", json!({ "command": text }))
}

fn read(path: &str) -> ToolCall {
    call("read_file", json!({ "path": path }))
}

/// What one call did, as the caller would say.
struct Done<'a> {
    call: ToolCall,
    output: &'a str,
    failed: bool,
    read_only: bool,
    /// The ledger's rows of the call; None when not read.
    edits: Option<Vec<(String, String)>>,
}

fn quiet(call: ToolCall) -> Done<'static> {
    Done {
        call,
        output: "done",
        failed: false,
        read_only: false,
        edits: Some(Vec::new()),
    }
}

fn edit(file: &str, state: &str) -> Done<'static> {
    Done {
        edits: Some(vec![(file.to_owned(), state.to_owned())]),
        ..quiet(command(&format!("write {file} {state}")))
    }
}

/// Runs `done` as its own step; the rung and count of its notice.
fn step(repeats: &mut Repeats, done: Done<'_>, limits: &Limits) -> Option<(RepeatRung, u32)> {
    repeats.step("t");
    let result = ToolResult {
        content: done.output.to_owned(),
        is_error: done.failed,
        exit_code: Some(i32::from(done.failed)),
        artifacts: Vec::new(),
    };
    let facts = Facts {
        read_only: done.read_only,
        edits: done.edits,
        ..Facts::default()
    };
    repeats
        .note("t", &done.call, &result, &result.content, &facts, limits)
        .map(|notice| (notice.rung, notice.count))
}

/// One edit, so the ledger is known to see this project, then commands that differ
/// and change nothing.
fn idle_after_edit(
    repeats: &mut Repeats,
    steps: usize,
    limits: &Limits,
) -> Vec<Option<RepeatRung>> {
    let mut rungs = vec![step(repeats, edit("src/lib.rs", "h1"), limits).map(|n| n.0)];
    for i in 0..steps {
        let done = quiet(command(&format!("echo try {i}")));
        rungs.push(step(repeats, done, limits).map(|n| n.0));
    }
    rungs
}

fn fired(rungs: &[Option<RepeatRung>]) -> Vec<(usize, RepeatRung)> {
    rungs
        .iter()
        .enumerate()
        .filter_map(|(at, rung)| rung.map(|rung| (at, rung)))
        .collect()
}

#[test]
fn steps_that_change_nothing_get_a_note_then_a_replan_and_a_stall_mark() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    assert_eq!(
        (limits.no_progress_note_at, limits.no_progress_replan_at),
        (12, 30)
    );
    let rungs = idle_after_edit(&mut repeats, 13, &limits);
    // The edit's step and 12 idle ones are behind the 14th call when it brings nothing.
    assert_eq!(fired(&rungs), [(13, RepeatRung::NoProgress)]);
    assert_eq!(
        repeats.take_stall("t", &limits).map(|stall| stall.count),
        None,
        "a note alone marks no stall"
    );
    let mut more = Vec::new();
    for i in 0..30 {
        let done = quiet(command(&format!("echo more {i}")));
        more.push(step(&mut repeats, done, &limits).map(|n| n.0));
    }
    // 30 idle steps are reached on the 31st call, the 18th after the note.
    let at = 30 - 13;
    assert_eq!(
        fired(&more),
        [
            (at, RepeatRung::NoProgressReplan),
            (at + 12, RepeatRung::NoProgressReplan)
        ]
    );
    assert_eq!(
        repeats.take_stall("t", &limits).map(|stall| stall.count),
        Some(42),
        "the judge hook gets the turn once"
    );
    assert_eq!(
        repeats.take_stall("t", &limits).map(|stall| stall.count),
        None
    );
    assert_eq!(
        repeats
            .take_stall("other turn", &limits)
            .map(|stall| stall.count),
        None
    );
}

#[test]
fn progress_starts_the_count_over() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let rungs = idle_after_edit(&mut repeats, 11, &limits);
    assert_eq!(fired(&rungs), []);
    assert_eq!(step(&mut repeats, edit("src/main.rs", "h2"), &limits), None);
    for i in 0..12 {
        let done = quiet(command(&format!("echo again {i}")));
        assert_eq!(step(&mut repeats, done, &limits), None, "step {i}");
    }
    let done = quiet(command("echo once more"));
    assert_eq!(
        step(&mut repeats, done, &limits).map(|n| n.0),
        Some(RepeatRung::NoProgress)
    );
}

#[test]
fn the_switch_turns_the_rule_off() {
    let mut repeats = Repeats::default();
    let mut limits = Limits::default();
    limits.no_progress = false;
    let rungs = idle_after_edit(&mut repeats, 60, &limits);
    assert_eq!(fired(&rungs), []);
    assert_eq!(
        repeats.take_stall("t", &limits).map(|stall| stall.count),
        None
    );
}

#[test]
fn an_edit_undone_and_done_again_is_no_news() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let mut rungs = Vec::new();
    for i in 0..20 {
        let state = if i % 2 == 0 { "h1" } else { "h0" };
        let done = Done {
            call: command(&format!("apply attempt {i}")),
            ..edit("src/lib.rs", state)
        };
        rungs.push(step(&mut repeats, done, &limits).map(|n| n.0));
    }
    // Two states are new; then 12 steps that only flip between them.
    assert_eq!(fired(&rungs).first(), Some(&(14, RepeatRung::NoProgress)));
}

#[test]
fn a_long_research_turn_is_not_nagged() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    for i in 0..120 {
        // Many files read alike, as empty or license headers do: a new file is research.
        let done = Done {
            output: "// SPDX-License-Identifier: MIT",
            read_only: true,
            edits: None,
            ..quiet(read(&format!("src/module_{i}.rs")))
        };
        assert_eq!(step(&mut repeats, done, &limits), None, "read {i}");
        let search = Done {
            output: "no matches",
            read_only: true,
            edits: None,
            ..quiet(call(
                "search_files",
                json!({ "pattern": format!("fn handler_{i}") }),
            ))
        };
        assert_eq!(step(&mut repeats, search, &limits), None, "search {i}");
    }
}

#[test]
fn batch_edits_are_progress() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    for i in 0..80 {
        // The same output each time; the ledger shows a new file state.
        let file = format!("src/file_{i}.rs");
        let done = Done {
            call: command(&format!("python3 rename.py {file}")),
            ..edit(&file, "renamed")
        };
        assert_eq!(step(&mut repeats, done, &limits), None, "edit {i}");
    }
}

#[test]
fn varied_retries_that_end_in_a_pass_are_progress() {
    let mut repeats = Repeats::default();
    let mut limits = Limits::default();
    limits.failure_ladder = false;
    let rungs = idle_after_edit(&mut repeats, 2, &limits);
    assert_eq!(fired(&rungs), []);
    let fail = Done {
        output: "error: linking failed",
        failed: true,
        ..quiet(command("make build"))
    };
    assert_eq!(step(&mut repeats, fail, &limits), None);
    for i in 0..8 {
        let done = quiet(command(&format!("echo retry {i}")));
        assert_eq!(step(&mut repeats, done, &limits), None);
    }
    // It passes with an output seen before: the pass alone is the news.
    let pass = quiet(command("make build LDFLAGS=-lm"));
    assert_eq!(step(&mut repeats, pass, &limits), None);
    for i in 0..12 {
        let done = quiet(command(&format!("echo after {i}")));
        assert_eq!(step(&mut repeats, done, &limits), None, "after {i}");
    }
}

#[test]
fn a_project_the_ledger_does_not_watch_is_not_nagged_for_writes() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    for i in 0..60 {
        // Too large to walk: no rows ever, so a new command may have written.
        let done = quiet(command(&format!("sed -i 's/{i}/x/' big.txt")));
        assert_eq!(step(&mut repeats, done, &limits), None, "write {i}");
    }
}

#[test]
fn a_note_comes_once_per_step_of_many_calls() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let _ = idle_after_edit(&mut repeats, 12, &limits);
    repeats.step("t");
    let mut rungs = Vec::new();
    for i in 0..4 {
        let result = ToolResult {
            exit_code: Some(0),
            ..ToolResult::text("done".into())
        };
        let facts = Facts {
            read_only: false,
            edits: Some(Vec::new()),
            ..Facts::default()
        };
        let call = command(&format!("echo batch {i}"));
        let notice = repeats.note("t", &call, &result, &result.content, &facts, &limits);
        rungs.push(notice.map(|n| n.rung));
    }
    assert_eq!(fired(&rungs), [(0, RepeatRung::NoProgress)]);
}

#[test]
fn a_write_the_ledger_cannot_see_is_progress() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let _ = idle_after_edit(&mut repeats, 11, &limits);
    // A subagent's report reads alike each time; the ledger never sees its writes.
    for i in 0..20 {
        let done = Done {
            output: "agent finished",
            edits: Some(Vec::new()),
            ..quiet(call("spawn_agent", json!({ "task": format!("part {i}") })))
        };
        assert_eq!(step(&mut repeats, done, &limits), None, "agent {i}");
    }
}

#[test]
fn looks_at_a_running_job_are_not_counted() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let _ = idle_after_edit(&mut repeats, 11, &limits);
    // A build in the background shows only a counter between looks.
    for i in 0..40 {
        let done = Done {
            output: &format!("job 1 running: [{i}/300] Compiling"),
            read_only: true,
            edits: None,
            ..quiet(call("jobs", json!({ "action": "wait", "job_id": "1" })))
        };
        assert_eq!(step(&mut repeats, done, &limits), None, "look {i}");
    }
    // The looks neither moved the count nor started it over.
    let done = quiet(command("echo one"));
    assert_eq!(step(&mut repeats, done, &limits), None);
    let done = quiet(command("echo two"));
    assert_eq!(
        step(&mut repeats, done, &limits).map(|n| n.0),
        Some(RepeatRung::NoProgress)
    );
}
