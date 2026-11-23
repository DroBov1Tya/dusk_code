//! Test runs with nothing changed between get a note with what the last run found; runs
//! after a change, other runners and other scopes after a pass get none. The failing ids
//! come from untrusted output.

use dusk_protocol::thread::RepeatRung;
use serde_json::json;

use super::{
    Facts, Repeats,
    test_runs::{failing_ids, runner},
};
use crate::{config::Limits, providers::ToolCall, tools::ToolResult};

fn command(text: &str) -> ToolCall {
    ToolCall {
        id: "call".into(),
        name: "run_command".into(),
        arguments: json!({ "command": text }),
    }
}

fn exited(code: i32, output: &str) -> ToolResult {
    ToolResult {
        content: format!("exit code: {code}\nstdout:\n{output}"),
        is_error: code != 0,
        exit_code: Some(code),
        artifacts: Vec::new(),
    }
}

/// The output of a cargo run where `failing` failed, with `noise` that differs per run.
fn cargo_output(failing: &[&str], noise: &str) -> String {
    let mut lines = vec![format!("running {} tests", failing.len() + 1)];
    lines.push("test tests::passes ... ok".into());
    for name in failing {
        lines.push(format!("test {name} ... FAILED"));
    }
    lines.push(format!("thread 'main' panicked at {noise}"));
    lines.push("test result: FAILED. 1 passed; 1 failed".into());
    lines.join("\n")
}

/// Runs `call` as its own step with the ledger at `ledger`; the notice's rung, count
/// and note.
fn run(
    repeats: &mut Repeats,
    call: &ToolCall,
    result: &ToolResult,
    ledger: Option<u64>,
    limits: &Limits,
) -> Option<(RepeatRung, u32, String)> {
    repeats.step("t");
    let facts = Facts {
        read_only: false,
        ledger,
    };
    repeats
        .note("t", call, result, &result.content, &facts, limits)
        .map(|notice| (notice.rung, notice.count, notice.note))
}

fn other(repeats: &mut Repeats, text: &str, read_only: bool, limits: &Limits) {
    repeats.step("t");
    let result = ToolResult::text(format!("output of {text}"));
    let facts = Facts {
        read_only,
        ledger: None,
    };
    repeats.note(
        "t",
        &command(text),
        &result,
        &result.content,
        &facts,
        limits,
    );
}

#[test]
fn a_test_run_repeating_one_with_nothing_changed_is_noted() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let failed = exited(
        101,
        &cargo_output(&["tests::adds", "tests::subtracts"], "a"),
    );
    assert!(
        run(
            &mut repeats,
            &command("cargo test"),
            &failed,
            Some(4),
            &limits
        )
        .is_none()
    );
    other(&mut repeats, "cat src/lib.rs", true, &limits);
    // The same failures again under another filter, with no file changed.
    let reshuffled = command("cargo test -- --test-threads=1 2>&1 | tail -40");
    let again = exited(
        101,
        &cargo_output(&["tests::subtracts", "tests::adds"], "b"),
    );
    let (rung, count, note) =
        run(&mut repeats, &reshuffled, &again, Some(4), &limits).expect("a note");
    assert_eq!((rung, count), (RepeatRung::TestRerun, 2));
    assert!(note.contains("`cargo test` run at step 1"), "{note}");
    assert!(
        note.contains("had 2 failures: tests::adds, tests::subtracts"),
        "{note}"
    );
    // Two more in a row ask for a plan.
    let third = run(&mut repeats, &reshuffled, &again, Some(4), &limits);
    assert_eq!(
        third.map(|(rung, count, _)| (rung, count)),
        Some((RepeatRung::TestRerun, 3))
    );
    let fourth = run(&mut repeats, &reshuffled, &again, Some(4), &limits).expect("a plan");
    assert_eq!((fourth.0, fourth.1), (RepeatRung::TestRerunReplan, 4));
    assert!(fourth.2.contains("different plan"), "{}", fourth.2);
}

#[test]
fn narrower_runs_to_look_at_each_failure_are_not_repeats() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let all = ["tests::adds", "tests::subtracts", "tests::divides"];
    let failed = exited(101, &cargo_output(&all, "a"));
    let suite = command("cargo test");
    assert!(run(&mut repeats, &suite, &failed, Some(4), &limits).is_none());
    for name in all {
        let focused = command(&format!("cargo test {name} -- --nocapture"));
        let output = exited(101, &cargo_output(&[name], "b"));
        let notice = run(&mut repeats, &focused, &output, Some(4), &limits);
        assert!(notice.is_none(), "{name}: {notice:?}");
    }
    // The whole suite again, still with nothing changed, repeats the first run.
    let (rung, _, note) = run(&mut repeats, &suite, &failed, Some(4), &limits).expect("a note");
    assert_eq!(rung, RepeatRung::TestRerun);
    assert!(note.contains("run at step 1"), "{note}");
}

#[test]
fn writes_the_ledger_cannot_see_are_changes() {
    let limits = Limits::default();
    let failed = exited(101, &cargo_output(&["tests::adds"], "a"));
    let test = command("cargo test");
    let delegate = ToolCall {
        id: "call".into(),
        name: "ask_agent".into(),
        arguments: json!({ "task": "fix tests::adds" }),
    };
    let start_job = ToolCall {
        id: "call".into(),
        name: "run_command".into(),
        arguments: json!({ "command": "./regen.sh", "background": true }),
    };
    let wait_job = ToolCall {
        id: "call".into(),
        name: "jobs".into(),
        arguments: json!({ "action": "wait" }),
    };
    for between in [
        vec![(&delegate, false)],
        vec![(&start_job, false)],
        vec![(&wait_job, true)],
    ] {
        let mut repeats = Repeats::default();
        // The ledger has seen this thread change a file: it is trusted.
        assert!(run(&mut repeats, &test, &failed, Some(1), &limits).is_none());
        if between[0].0.name == "jobs" {
            repeats.step("t");
            let started = ToolResult::text("background job started: job_id=1".into());
            let facts = Facts::default();
            repeats.note("t", &start_job, &started, &started.content, &facts, &limits);
            assert!(run(&mut repeats, &test, &failed, Some(1), &limits).is_none());
        }
        for (call, read_only) in between {
            repeats.step("t");
            let result = ToolResult::text("done".into());
            let facts = Facts {
                read_only,
                ledger: None,
            };
            repeats.note("t", call, &result, &result.content, &facts, &limits);
        }
        // The same failure may climb the ladder; the re-run rule says nothing.
        let rung = run(&mut repeats, &test, &failed, Some(1), &limits).map(|notice| notice.0);
        assert!(
            matches!(rung, None | Some(RepeatRung::SameError)),
            "{rung:?}"
        );
    }
}

#[test]
fn a_change_the_ledger_saw_starts_the_count_over() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let failed = exited(101, &cargo_output(&["tests::adds"], "a"));
    let test = command("cargo test");
    for ledger in [3, 5, 6, 9] {
        other(
            &mut repeats,
            &format!("sed -i 's/{ledger}/b/' src/lib.rs"),
            false,
            &limits,
        );
        let notice = run(&mut repeats, &test, &failed, Some(ledger), &limits);
        // The same failure still climbs the ladder; the re-run rule says nothing.
        let rung = notice.map(|notice| notice.0);
        assert!(
            matches!(rung, None | Some(RepeatRung::SameError)),
            "{rung:?}"
        );
    }
}

#[test]
fn without_a_ledger_to_trust_any_call_that_may_write_is_a_change() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let failed = exited(1, "FAILED tests/test_a.py::test_x - assert 1 == 2");
    let test = command("python -m pytest -q");
    // The ledger recorded nothing in this thread: a project too large to walk looks so.
    assert!(run(&mut repeats, &test, &failed, Some(0), &limits).is_none());
    other(
        &mut repeats,
        "sed -i 's/1/2/' tests/test_a.py",
        false,
        &limits,
    );
    assert!(run(&mut repeats, &test, &failed, Some(0), &limits).is_none());
    // Only reads between: nothing could have changed.
    other(&mut repeats, "cat tests/test_a.py", true, &limits);
    let (rung, _, note) = run(&mut repeats, &test, &failed, None, &limits).expect("a note");
    assert_eq!(rung, RepeatRung::TestRerun);
    assert!(note.contains("`pytest`"), "{note}");
    assert!(note.contains("tests/test_a.py::test_x"), "{note}");
}

#[test]
fn another_scope_after_a_pass_is_ordinary_but_the_same_command_is_not() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let passed = exited(0, "test result: ok. 3 passed");
    for scope in ["core", "protocol", "server", "evals"] {
        let test = command(&format!("cargo test -p dusk_{scope}"));
        assert!(
            run(&mut repeats, &test, &passed, Some(2), &limits).is_none(),
            "{scope}"
        );
    }
    let again = command("cargo test -p dusk_evals");
    let (rung, _, note) = run(&mut repeats, &again, &passed, Some(2), &limits).expect("a note");
    assert_eq!(rung, RepeatRung::TestRerun);
    assert!(note.contains("it passed"), "{note}");
    // Another runner keeps its own count.
    let npm = command("npm test");
    assert!(run(&mut repeats, &npm, &passed, Some(2), &limits).is_none());
}

#[test]
fn with_the_switch_off_test_runs_get_no_note() {
    let mut repeats = Repeats::default();
    let mut limits = Limits::default();
    limits.test_reruns = false;
    limits.failure_ladder = false;
    let failed = exited(101, &cargo_output(&["tests::adds"], "a"));
    for noise in ["a", "b", "c", "d"] {
        let output = exited(101, &cargo_output(&["tests::adds"], noise));
        let test = command(&format!("cargo test {noise}"));
        assert!(run(&mut repeats, &test, &output, Some(1), &limits).is_none());
    }
    assert!(
        run(
            &mut repeats,
            &command("cargo test"),
            &failed,
            Some(1),
            &limits
        )
        .is_none()
    );
}

#[test]
fn the_same_failing_tests_climb_the_ladder_whatever_else_the_output_says() {
    let mut repeats = Repeats::default();
    let limits = Limits::default();
    let mut rungs = Vec::new();
    for (round, noise) in ["src/a.rs:1", "src/b.rs:9", "src/c.rs:4"]
        .iter()
        .enumerate()
    {
        other(
            &mut repeats,
            &format!("sed -i 's/{round}/x/' src/lib.rs"),
            false,
            &limits,
        );
        let output = exited(
            101,
            &cargo_output(&["tests::adds", "tests::subtracts"], noise),
        );
        let test = command(&format!("cargo test --test t{round}"));
        let ledger = Some(10 + round as u64);
        rungs.push(run(&mut repeats, &test, &output, ledger, &limits).map(|n| (n.0, n.1)));
    }
    assert_eq!(rungs, [None, None, Some((RepeatRung::SameError, 3))]);
    // Another set of failures is another problem.
    let other_set = exited(101, &cargo_output(&["tests::divides"], "x"));
    let notice = run(
        &mut repeats,
        &command("cargo test"),
        &other_set,
        Some(20),
        &limits,
    );
    assert!(notice.is_none());
}

#[test]
fn test_runners_are_told_from_other_commands() {
    for (line, expected) in [
        ("cargo test", "cargo test"),
        ("cargo +1.99.1 test -p core -- --nocapture", "cargo test"),
        (
            "cd core && RUST_LOG=1 cargo t 2>&1 | tail -50",
            "cargo test",
        ),
        ("cargo nextest run", "cargo nextest"),
        ("pytest -x tests/a.py", "pytest"),
        ("python3 -m pytest -q", "pytest"),
        ("uv run pytest", "pytest"),
        ("python -m unittest discover", "python unittest"),
        ("go test ./...", "go test"),
        ("npm test", "npm test"),
        ("pnpm run test:unit", "pnpm run test:unit"),
        ("npx vitest run", "vitest"),
        ("bundle exec rspec spec/a_spec.rb", "rspec"),
        ("make -j4 check", "make test"),
        ("./gradlew test", "gradlew test"),
        ("timeout 600 dotnet test", "dotnet test"),
    ] {
        assert_eq!(runner(line).as_deref(), Some(expected), "{line}");
    }
    for line in [
        "cargo build",
        "cargo check --tests",
        "cat tests/a.py",
        "npm run build",
        "npm install",
        "go build ./...",
        "make",
        "python script.py",
        "grep -rn test src",
        "",
        "'unclosed",
    ] {
        assert_eq!(runner(line), None, "{line}");
    }
}

#[test]
fn failing_ids_are_read_from_each_runners_output() {
    let output = "\
test tests::adds ... FAILED
test tests::passes ... ok
        FAIL [   0.004s] core tests::nexted
FAILED tests/test_a.py::test_x - assert 1 == 2
ERROR tests/test_b.py - ImportError
tests/test_c.py::test_y FAILED [ 50%]
--- FAIL: TestDivide (0.00s)
FAIL\texample.com/calc\t0.01s
FAIL src/a.test.js (5.2 s)
 FAIL  src/b.test.ts > math > adds
rspec ./spec/a_spec.rb:12 # adds
test result: FAILED. 1 passed; 1 failed
ERROR: something else
FAIL
";
    assert_eq!(
        failing_ids(output),
        [
            "./spec/a_spec.rb:12",
            "TestDivide",
            "core tests::nexted",
            "example.com/calc",
            "src/a.test.js",
            "src/b.test.ts > math > adds",
            "tests/test_a.py::test_x",
            "tests/test_b.py",
            "tests/test_c.py::test_y",
            "tests::adds",
        ]
    );
}

#[test]
fn failing_ids_stay_bounded_on_any_output() {
    let many: String = (0..1000)
        .map(|n| format!("test t{n} ... FAILED\n"))
        .collect();
    assert_eq!(failing_ids(&many).len(), 64);
    let long = format!("test {} ... FAILED", "x".repeat(10_000));
    assert!(failing_ids(&long)[0].chars().count() <= 201);
    for odd in [
        "",
        "\u{0}",
        "FAIL [",
        "FAIL [\u{e9}",
        "test  ... FAILED",
        "--- FAIL: ",
        "ééé",
    ] {
        failing_ids(odd);
    }
    let huge = "x".repeat(3 << 20) + "\ntest tail::id ... FAILED";
    assert_eq!(failing_ids(&huge), ["tail::id"]);
}
