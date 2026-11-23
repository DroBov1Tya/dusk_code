//! Calls going in circles get a note, then a re-plan; work that moves on gets none.

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

/// Runs `call` as its own step; the rung and count of its notice.
fn step(
    repeats: &mut Repeats,
    call: &ToolCall,
    output: &str,
    read_only: bool,
    limits: &Limits,
) -> Option<(RepeatRung, u32)> {
    repeats.step("t");
    ran(repeats, call, output, read_only, limits)
}

/// Notes `call` in the step already running.
fn ran(
    repeats: &mut Repeats,
    call: &ToolCall,
    output: &str,
    read_only: bool,
    limits: &Limits,
) -> Option<(RepeatRung, u32)> {
    let result = ToolResult::text(output.into());
    let facts = Facts {
        read_only,
        ledger: None,
    };
    repeats
        .note("t", call, &result, &result.content, &facts, limits)
        .map(|notice| (notice.rung, notice.count))
}

#[test]
fn two_calls_taking_turns_with_the_same_results_are_a_cycle() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let edit = command("sed -i 's/a/b/' src/lib.rs");
    let check = command("cargo check");
    let mut rungs = Vec::new();
    for _ in 0..5 {
        rungs.push(step(&mut repeats, &edit, "", false, &limits));
        rungs.push(step(
            &mut repeats,
            &check,
            "error[E0308]: mismatched types",
            false,
            &limits,
        ));
    }
    use RepeatRung::{Cycle, CycleReplan};
    assert_eq!(
        rungs,
        [
            None,
            None,
            None,
            None,
            None,
            Some((Cycle, 3)),
            Some((Cycle, 3)),
            Some((Cycle, 4)),
            Some((Cycle, 4)),
            Some((CycleReplan, 5)),
        ]
    );
}

#[test]
fn the_same_call_in_a_row_is_noted_at_three_and_asks_for_a_plan_at_five() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let status = command("git status");
    let rungs: Vec<_> = (0..5)
        .map(|_| step(&mut repeats, &status, "nothing to commit", false, &limits))
        .collect();
    use RepeatRung::{CycleReplan, SameCall};
    assert_eq!(
        rungs,
        [
            None,
            None,
            Some((SameCall, 3)),
            Some((SameCall, 4)),
            Some((CycleReplan, 5))
        ]
    );
}

#[test]
fn a_read_that_keeps_giving_the_same_result_with_nothing_changed_is_noted() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let main = read("src/main.rs");
    assert_eq!(
        step(&mut repeats, &main, "fn main() {}", true, &limits),
        None
    );
    assert_eq!(step(&mut repeats, &read("a.rs"), "a", true, &limits), None);
    assert_eq!(
        step(&mut repeats, &main, "fn main() {}", true, &limits),
        None
    );
    assert_eq!(step(&mut repeats, &read("b.rs"), "b", true, &limits), None);
    assert_eq!(
        step(&mut repeats, &main, "fn main() {}", true, &limits),
        Some((RepeatRung::Cycle, 3))
    );
}

#[test]
fn a_read_again_after_a_call_that_may_write_is_new() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let main = read("src/main.rs");
    for round in 0..6 {
        assert_eq!(
            step(&mut repeats, &main, "fn main() {}", true, &limits),
            None
        );
        let edit = command(&format!("sed -i 's/x{round}/y/' src/main.rs"));
        assert_eq!(step(&mut repeats, &edit, "", false, &limits), None);
    }
}

#[test]
fn varied_retries_batch_edits_and_checks_between_are_not_a_cycle() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    for attempt in 1..=10 {
        let retry = command(&format!("make -j{attempt}"));
        assert_eq!(
            step(&mut repeats, &retry, "make: *** no rule", false, &limits),
            None
        );
    }
    for file in 0..12 {
        let edit = command(&format!("sed -i 's/old/new/' src/file{file}.rs"));
        assert_eq!(step(&mut repeats, &edit, "", false, &limits), None);
        let check = command("cargo check");
        let done = "Finished `dev` profile in 2.31s";
        assert_eq!(step(&mut repeats, &check, done, false, &limits), None);
    }
}

#[test]
fn a_research_turn_reading_many_files_gets_no_note() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    for group in 0..8 {
        repeats.step("t");
        for file in 0..4 {
            let path = format!("src/m{group}/f{file}.rs");
            assert_eq!(ran(&mut repeats, &read(&path), &path, true, &limits), None);
        }
        let search = call("search_files", json!({ "pattern": format!("fn f{group}") }));
        assert_eq!(
            step(&mut repeats, &search, "src/a.rs:1", true, &limits),
            None
        );
    }
    // Coming back to a file once more is ordinary.
    let again = read("src/m0/f0.rs");
    assert_eq!(
        step(&mut repeats, &again, "src/m0/f0.rs", true, &limits),
        None
    );
}

#[test]
fn the_reads_of_one_step_count_as_one() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let mut notices = Vec::new();
    for _ in 0..3 {
        repeats.step("t");
        for path in ["a.rs", "b.rs", "c.rs"] {
            notices.push(ran(&mut repeats, &read(path), path, true, &limits));
        }
    }
    // Only the last read of the third step completes the third identical step.
    let mut expected = vec![None; 8];
    expected.push(Some((RepeatRung::SameCall, 3)));
    assert_eq!(notices, expected);
}

#[test]
fn times_and_ids_in_a_result_do_not_hide_a_repeat_but_counts_do() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let jobs = call("jobs", json!({}));
    let at = |time: &str, id: &str| format!("job {id} running since 10:{time}, 0.{time}s");
    assert_eq!(
        step(&mut repeats, &jobs, &at("01", "a81f3c9"), false, &limits),
        None
    );
    assert_eq!(
        step(&mut repeats, &jobs, &at("02", "b92e4d0"), false, &limits),
        None
    );
    assert_eq!(
        step(&mut repeats, &jobs, &at("03", "c03f5e1"), false, &limits),
        Some((RepeatRung::SameCall, 3))
    );
    let mut repeats = Repeats::default();
    let count = command("wc -l log.txt");
    for lines in 10..16 {
        let output = format!("{lines} log.txt");
        assert_eq!(step(&mut repeats, &count, &output, false, &limits), None);
    }
}

#[test]
fn with_cycles_off_only_the_same_call_in_a_row_is_noted() {
    let mut repeats = Repeats::default();
    let mut limits = Limits::default();
    limits.loop_cycles = false;
    let edit = command("sed -i 's/a/b/' src/lib.rs");
    let check = command("cargo check");
    for _ in 0..6 {
        assert_eq!(step(&mut repeats, &edit, "", false, &limits), None);
        assert_eq!(step(&mut repeats, &check, "error", false, &limits), None);
    }
    let status = command("git status");
    let rungs: Vec<_> = (0..6)
        .map(|_| step(&mut repeats, &status, "clean", false, &limits))
        .map(|notice| notice.map(|(rung, _)| rung))
        .collect();
    let same = Some(RepeatRung::SameCall);
    assert_eq!(rungs, [None, None, same, same, same, same]);
}

#[test]
fn any_result_is_hashed_without_panic() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let huge = "é1:2 0x7f a81f3c9\n".repeat(50_000);
    for output in ["", "\u{0}", "::::", "1.2.3:4", &huge, "日本語:12"] {
        step(&mut repeats, &command("x"), output, false, &limits);
    }
}
