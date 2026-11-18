//! Which pass ends a loop, as `solved` decides it, and the lesson line.

use serde_json::json;

use super::{Repeats, Solved, lesson, lesson_note, signature::invocation};
use crate::{config::Limits, providers::ToolCall, tools::ToolResult};

const UNKNOWN: &str = "error[E0425]: cannot find value `limit` in this scope";

fn command(text: &str) -> ToolCall {
    ToolCall {
        id: "call".into(),
        name: "run_command".into(),
        arguments: json!({ "command": text }),
    }
}

fn exited(code: i32, output: &str) -> ToolResult {
    ToolResult {
        content: format!("exit code: {code}\nstderr:\n{output}"),
        is_error: code != 0,
        exit_code: Some(code),
        artifacts: Vec::new(),
    }
}

fn episodes_on() -> Limits {
    let mut limits = Limits::default();
    limits.episodes = true;
    limits
}

fn fail(repeats: &mut Repeats, text: &str, error: &str, limits: &Limits) {
    let result = exited(101, error);
    repeats.note("t", &command(text), &result, &result.content, limits);
}

fn pass(repeats: &mut Repeats, text: &str) -> Option<Solved> {
    repeats.solved("t", &command(text), &exited(0, "ok"))
}

#[test]
fn two_failures_then_a_pass_of_the_same_program_end_a_loop() {
    let mut repeats = Repeats::default();
    let limits = episodes_on();
    fail(&mut repeats, "cargo test -p core", UNKNOWN, &limits);
    fail(&mut repeats, "cargo test -p core --lib", UNKNOWN, &limits);
    let solved = pass(&mut repeats, "cargo test -p core").expect("a loop ended");
    assert_eq!(solved.count, 2);
    assert_eq!(solved.passed, "cargo test -p core");
    assert_eq!(
        solved.attempts,
        ["cargo test -p core", "cargo test -p core --lib"]
    );
    assert!(solved.problem.contains("cannot find value"), "{solved:?}");
    assert!(solved.key.starts_with("run_command\ncargo\n"), "{solved:?}");
    assert!(solved.since > 0);
    assert_eq!(pass(&mut repeats, "cargo test -p core"), None, "ended once");
}

#[test]
fn one_failure_then_a_pass_is_ordinary_work() {
    let mut repeats = Repeats::default();
    let limits = episodes_on();
    fail(&mut repeats, "cargo test", UNKNOWN, &limits);
    assert_eq!(pass(&mut repeats, "cargo test"), None);
    // The pass ended that failure: one more is a new first one.
    fail(&mut repeats, "cargo test", UNKNOWN, &limits);
    assert_eq!(pass(&mut repeats, "cargo test"), None);
}

#[test]
fn a_pass_of_another_program_or_argument_ends_nothing() {
    let mut repeats = Repeats::default();
    let limits = episodes_on();
    fail(&mut repeats, "cargo test", UNKNOWN, &limits);
    fail(&mut repeats, "cargo test --all", UNKNOWN, &limits);
    assert_eq!(pass(&mut repeats, "npm test"), None);
    assert_eq!(pass(&mut repeats, "cargo build"), None);
    let other_tool = ToolCall {
        id: "read".into(),
        name: "read_file".into(),
        arguments: json!({ "path": "src/lib.rs" }),
    };
    let read = ToolResult::text("fn main() {}".into());
    assert_eq!(repeats.solved("t", &other_tool, &read), None);
    // A background start has no exit code, so it says nothing of the loop.
    let started = ToolResult::text("started job 1".into());
    assert_eq!(repeats.solved("t", &command("cargo test"), &started), None);
    assert!(pass(&mut repeats, "cargo test").is_some(), "still open");
}

#[test]
fn only_the_error_that_failed_last_makes_an_episode() {
    let mut repeats = Repeats::default();
    let limits = episodes_on();
    let other = "error: linker `cc` not found";
    fail(&mut repeats, "cargo test", UNKNOWN, &limits);
    fail(&mut repeats, "cargo test", UNKNOWN, &limits);
    fail(&mut repeats, "cargo test -q", other, &limits);
    fail(&mut repeats, "cargo test -v", other, &limits);
    let solved = pass(&mut repeats, "cargo test").expect("a loop ended");
    assert!(solved.problem.contains("linker"), "{solved:?}");
    assert_eq!(pass(&mut repeats, "cargo test"), None, "both were ended");
}

#[test]
fn a_turn_makes_three_episodes_at_most() {
    let mut repeats = Repeats::default();
    let limits = episodes_on();
    let loops = [
        ("cargo test", "error: could not compile `core`"),
        ("npm run lint", "error: unexpected token in app.ts"),
        ("make all", "make: *** No rule to make target 'all'.  Stop."),
        (
            "pytest tests",
            "ModuleNotFoundError: No module named 'yaml'",
        ),
    ];
    let mut solved = 0;
    for (text, error) in loops {
        fail(&mut repeats, text, error, &limits);
        fail(&mut repeats, text, error, &limits);
        solved += usize::from(pass(&mut repeats, text).is_some());
    }
    assert_eq!(solved, 3);
    // A new turn has its own three.
    let result = exited(101, UNKNOWN);
    repeats.note(
        "u",
        &command("cargo test"),
        &result,
        &result.content,
        &limits,
    );
    repeats.note(
        "u",
        &command("cargo test"),
        &result,
        &result.content,
        &limits,
    );
    let ok = exited(0, "ok");
    assert!(repeats.solved("u", &command("cargo test"), &ok).is_some());
}

#[test]
fn with_both_switches_off_nothing_is_counted() {
    let mut repeats = Repeats::default();
    let off = Limits::default();
    fail(&mut repeats, "cargo test", UNKNOWN, &off);
    fail(&mut repeats, "cargo test", UNKNOWN, &off);
    // Asked anyway, the pass finds no count to end.
    assert_eq!(pass(&mut repeats, "cargo test"), None);
}

#[test]
fn the_ladder_alone_counts_for_its_notes_as_before() {
    let mut repeats = Repeats::default();
    let mut ladder = Limits::default();
    ladder.failure_ladder = true;
    let result = exited(101, UNKNOWN);
    let mut counts = Vec::new();
    for _ in 0..3 {
        let notice = repeats.note("t", &command("cargo test"), &result, "x", &ladder);
        counts.push(notice.map(|notice| notice.count));
    }
    assert_eq!(counts, [None, None, Some(3)]);
}

#[test]
fn the_program_and_its_first_argument() {
    let cases = [
        ("cargo +1.99.1 test -p core", "cargo", Some("+1.99.1")),
        (
            "cd x && sudo -E npm run build 2>&1 | tail",
            "npm",
            Some("run"),
        ),
        ("make -j4", "make", None),
        ("RUST_LOG=debug \"cargo\" 'test'", "cargo", Some("test")),
    ];
    for (command, program, first) in cases {
        let found = invocation(command).expect(command);
        assert_eq!(
            (found.0.as_str(), found.1.as_deref()),
            (program, first),
            "{command}"
        );
    }
    assert_eq!(invocation("cd x"), None);
}

#[test]
fn the_lesson_note_is_one_short_line() {
    let note = lesson_note(3);
    assert_eq!(
        note,
        "[this passed after the same error came back 3 times. End your answer with one \
         line: Lesson: <what was wrong, under 20 words>]"
    );
    assert_eq!(note.len(), 125);
}

#[test]
fn the_lesson_is_the_last_line_that_starts_with_it() {
    let answer = "Fixed.\nLesson: first\n\n  Lesson:  the toolchain was missing  \nThanks.";
    assert_eq!(lesson(answer), Some("the toolchain was missing"));
    assert_eq!(
        lesson("Fixed, no lesson here.\nA lesson: not at the start"),
        None
    );
    assert_eq!(
        lesson("Lesson: one\nLesson:   "),
        None,
        "the last one is empty"
    );
}
