//! Which pass ends a loop, as `solved` decides it, and the lesson line.

use std::path::Path;

use serde_json::json;

use super::{Repeats, Solved, lesson, lesson_note, signature::invocation};
use crate::{
    config::Limits,
    project::episodes::{Facts, Origin, record},
    providers::ToolCall,
    state::State,
    tools::ToolResult,
};

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
    repeats.note(
        "t",
        &command(text),
        &result,
        &result.content,
        &super::Facts::default(),
        limits,
    );
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
        &super::Facts::default(),
        &limits,
    );
    repeats.note(
        "u",
        &command("cargo test"),
        &result,
        &result.content,
        &super::Facts::default(),
        &limits,
    );
    let ok = exited(0, "ok");
    assert!(repeats.solved("u", &command("cargo test"), &ok).is_some());
}

#[test]
fn with_both_switches_off_nothing_is_counted() {
    let mut repeats = Repeats::default();
    let mut off = Limits::default();
    off.failure_ladder = false;
    off.episodes = false;
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
    // The test re-run note would win the second run; this is about the ladder's count.
    ladder.test_reruns = false;
    let result = exited(101, UNKNOWN);
    let mut counts = Vec::new();
    for _ in 0..3 {
        let notice = repeats.note(
            "t",
            &command("cargo test"),
            &result,
            "x",
            &super::Facts::default(),
            &ladder,
        );
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

/// The project the episode tests record into.
const PROJECT: &str = "/work/service-a";

/// Fails `failing` twice, passes `passing`, and records the loop it ended as the agent
/// does; every text column of what was stored, or None when nothing was.
async fn recorded(
    variables: &[(&str, &str)],
    failing: &str,
    error: &str,
    passing: &str,
) -> Option<Vec<String>> {
    let state = State::in_memory().await.unwrap();
    state.create_project("p", "P", "", PROJECT).await.unwrap();
    for (name, value) in variables {
        state.set_project_env("p", name, value).await.unwrap();
    }
    let mut repeats = Repeats::default();
    let limits = episodes_on();
    fail(&mut repeats, failing, error, &limits);
    fail(&mut repeats, failing, error, &limits);
    let solved = pass(&mut repeats, passing).expect("a loop ended");
    let facts = Facts::Fixed {
        key: &solved.key,
        problem: &solved.problem,
        attempts: &solved.attempts,
        passed: &solved.passed,
        changed: &[],
        lesson: None,
    };
    let origin = Origin {
        project: Path::new(PROJECT),
        thread_id: None,
        turn_id: None,
        tool: "run_command",
        model: "m",
    };
    record(&state, &origin, &facts).await.unwrap()?;
    let row: (String, String, String) =
        sqlx::query_as("SELECT problem, failed, worked FROM episodes")
            .fetch_one(&state.pool)
            .await
            .unwrap();
    Some(vec![row.0, row.1, row.2])
}

/// Whether any 8 characters in a row of `secret` are in `stored`.
fn leaks(stored: &[String], secret: &str) -> bool {
    let chars: Vec<char> = secret.chars().collect();
    chars.windows(8).any(|piece| {
        let piece: String = piece.iter().collect();
        stored.iter().any(|text| text.contains(&piece))
    })
}

/// The command the review found: the ladder lists a call cut at 100 characters, and 21
/// characters of the key were left, too few for the rules to know them as a key. An
/// episode is built from the whole texts and cut only once cleaned.
const AWS_KEY: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";

fn sets_the_key() -> String {
    format!(
        "cd /work/service-a && AWS_PROFILE=prod aws configure set aws_secret_access_key {AWS_KEY}"
    )
}

#[tokio::test]
async fn a_key_the_listed_attempt_cut_is_cleaned_whole() {
    let stored = recorded(&[], &sets_the_key(), UNKNOWN, "aws configure list")
        .await
        .expect("written");
    assert!(stored[1].contains("aws_secret_access_key"), "{stored:?}");
    assert!(!leaks(&stored, AWS_KEY), "{stored:?}");
}

#[tokio::test]
async fn a_variable_the_listed_attempt_cut_is_cleaned_whole() {
    let value = "correct-horse-battery-staple";
    let passing = format!(
        "aws configure list --profile deploy-production-account-in-eu-west-1-cluster-a {value}"
    );
    assert!(passing.find(value).unwrap() < 100 && passing.len() > 100);
    let stored = recorded(&[("DEPLOY", value)], &sets_the_key(), UNKNOWN, &passing)
        .await
        .expect("written");
    assert!(stored[2].contains("<secret>"), "{stored:?}");
    assert!(!leaks(&stored, value), "{stored:?}");
}

#[tokio::test]
async fn a_key_the_error_excerpt_cut_is_cleaned_whole() {
    let run = "Zm9vYmFyQmF6UXV4MTIzNDU2Nzg5MGFiY2RlZg";
    let error = format!("error: {} {run} was refused", "word ".repeat(75));
    let at = error.find(run).unwrap();
    assert!(
        at + 8 < 400 && at + 24 > 400,
        "the old cut leaves part of the key"
    );
    let stored = recorded(
        &[],
        "aws configure get region",
        &error,
        "aws configure list",
    )
    .await
    .expect("written");
    assert!(!leaks(&stored, run), "{stored:?}");
}

/// A command that reads a secret file makes no episode, wherever in the line it is.
#[tokio::test]
async fn a_secret_file_past_the_listed_length_writes_nothing() {
    let failing = "cd /work/service-a/deployments/production/eu-west-1/cluster-a/services/billing-api-v2 \
                   && set -a && source ./config/.env && ./scripts/migrate";
    assert!(failing.find(".env").unwrap() > 100);
    let passing = "cd /work/service-a && ./scripts/migrate";
    assert_eq!(recorded(&[], failing, UNKNOWN, passing).await, None);
}
