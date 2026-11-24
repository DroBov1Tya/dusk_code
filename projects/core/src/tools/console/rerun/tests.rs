//! The narrow command on output captured from the runners: the line is there and exact
//! when the command line and the output leave no doubt, and absent whenever they do.

use super::rerun;
use crate::tools::console::command_line::{Reading, split};

const CARGO_TEST: &str = include_str!("../shape/fixtures/cargo_test.txt");
const PYTEST: &str = include_str!("../shape/fixtures/pytest.txt");
const PYTEST_VERBOSE: &str = include_str!("../shape/fixtures/pytest_verbose.txt");
const NPM_TEST: &str = include_str!("../shape/fixtures/npm_test.txt");

/// The command of the line `rerun` gives.
fn narrow(command: &str, output: &str) -> Option<String> {
    let line = rerun(command, output)?;
    let narrow = line
        .strip_prefix("Re-run only the failing tests: `")
        .and_then(|rest| rest.strip_suffix('`'))
        .unwrap_or_else(|| panic!("{line}"));
    Some(narrow.to_owned())
}

/// The words of the last command `line` runs, as `sh` would pass them.
fn words(line: &str) -> Vec<String> {
    split(line, Reading::Strict)
        .and_then(|split| split.commands.last().cloned())
        .unwrap_or_else(|| panic!("{line} does not read"))
}

#[test]
fn cargo_test_gets_the_failing_tests_by_exact_name() {
    for (command, expected) in [
        (
            "cargo +1.99.1 test",
            "cargo +1.99.1 test -- --exact tests::another_failure tests::fails_on_purpose",
        ),
        (
            "cd projects/core && cargo test -p fixcrate --lib 2>&1",
            "cd projects/core && cargo test -p fixcrate --lib 2>&1 -- --exact \
             tests::another_failure tests::fails_on_purpose",
        ),
        (
            "RUST_BACKTRACE=1 cargo test --features 'a b' -- --nocapture",
            "RUST_BACKTRACE=1 cargo test --features 'a b' -- --nocapture --exact \
             tests::another_failure tests::fails_on_purpose",
        ),
        (
            "cargo test -q -- --exact --test-threads=1",
            "cargo test -q -- --exact --test-threads=1 tests::another_failure \
             tests::fails_on_purpose",
        ),
    ] {
        assert_eq!(narrow(command, CARGO_TEST).as_deref(), Some(expected));
    }
    // A should_panic test is named without the marker libtest prints after it.
    let output = "test a::boom - should panic ... FAILED\n\
                  test result: FAILED. 3 passed; 1 failed; 0 ignored\n";
    assert_eq!(
        narrow("cargo test", output).as_deref(),
        Some("cargo test -- --exact a::boom")
    );
}

#[test]
fn cargo_test_gets_no_line_when_anything_is_unsure() {
    for command in [
        // The line's own filter would be added to ours, not narrowed.
        "cargo test parse",
        "cargo test -- parse",
        "cargo test -- --skip slow",
        // Not a test run, or not one line.
        "cargo bench",
        "cargo build",
        "cargo test | tail",
        "cargo test # all",
        "cargo test\ncargo test",
    ] {
        assert_eq!(rerun(command, CARGO_TEST), None, "{command}");
    }
    for output in [
        // A failure is counted that no line names: the output was cut.
        "test a ... FAILED\ntest result: FAILED. 1 passed; 2 failed; 0 ignored\n",
        // No count at all.
        "test a ... FAILED\n",
        // A doc test's name has spaces.
        "test src/lib.rs - add (line 5) ... FAILED\n\
         test result: FAILED. 0 passed; 1 failed; 0 ignored\n",
        // A build error: nothing ran.
        "error[E0425]: cannot find value `x` in this scope\n --> src/lib.rs:3:5\n",
        // Everything passed.
        "test a ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored\n",
        "",
    ] {
        assert_eq!(rerun("cargo test", output), None, "{output}");
    }
}

/// A cut that took a whole failing block, its count with it, leaves names and counts that
/// still agree: the cut alone must keep the line out.
#[test]
fn output_cut_in_the_middle_gets_no_line() {
    let whole = "running 2 tests\ntest a::fine ... ok\ntest b::bad ... FAILED\n\
                 test result: FAILED. 1 passed; 1 failed; 0 ignored\n";
    assert_eq!(
        narrow("cargo test --workspace --no-fail-fast", whole).as_deref(),
        Some("cargo test --workspace --no-fail-fast -- --exact b::bad")
    );
    let cut = whole.replace(
        "test a::fine ... ok\n",
        "test a::fine ... ok\n[40960 bytes cut]\n",
    );
    assert_eq!(rerun("cargo test --workspace --no-fail-fast", &cut), None);
    let pytest = PYTEST.replacen('\n', "\n[512 bytes cut]\n", 1);
    assert_eq!(rerun("pytest", &pytest), None);
    // A test may print a line that only looks like the marker.
    let printed = whole.replace(
        "test a::fine ... ok\n",
        "test a::fine ... ok\n[some bytes cut]\n",
    );
    assert!(rerun("cargo test", &printed).is_some());
}

#[test]
fn a_long_test_name_or_too_many_get_no_line() {
    let long = format!(
        "test a::{} ... FAILED\ntest result: FAILED. 0 passed; 1 failed; 0 ignored\n",
        "x".repeat(300)
    );
    assert_eq!(rerun("cargo test", &long), None);
    let mut many: String = (0..11)
        .map(|i| format!("test a::t{i} ... FAILED\n"))
        .collect();
    many.push_str("test result: FAILED. 0 passed; 11 failed; 0 ignored\n");
    assert_eq!(rerun("cargo test", &many), None);
}

#[test]
fn pytest_gets_the_failing_node_ids_in_place_of_the_paths() {
    for (command, output, expected) in [
        (
            "python -m pytest",
            PYTEST,
            "python -m pytest tests/test_checkout.py::test_checkout_total \
             tests/test_pricing.py::test_bulk_price",
        ),
        (
            "pytest tests -x -k 'not slow'",
            PYTEST,
            "pytest -x -k 'not slow' tests/test_checkout.py::test_checkout_total \
             tests/test_pricing.py::test_bulk_price",
        ),
        (
            "cd shop && uv run pytest -v tests/test_cart.py",
            PYTEST_VERBOSE,
            "cd shop && uv run pytest -v tests/test_cart.py::test_remove_missing",
        ),
        (
            "pytest -ra --cov src tests/",
            PYTEST_VERBOSE,
            "pytest -ra --cov src tests/test_cart.py::test_remove_missing",
        ),
    ] {
        assert_eq!(
            narrow(command, output).as_deref(),
            Some(expected),
            "{command}"
        );
    }
    // A parameter id is quoted, so the shell neither globs nor splits it.
    let output = "==== short test summary info ====\n\
                  FAILED t/test_a.py::test_x[0-a] - assert 1 == 2\n\
                  ==== 1 failed, 3 passed in 0.10s ====\n";
    let line = narrow("pytest -q", output).unwrap();
    assert_eq!(line, "pytest -q 't/test_a.py::test_x[0-a]'");
    assert_eq!(words(&line), ["pytest", "-q", "t/test_a.py::test_x[0-a]"]);
}

#[test]
fn pytest_gets_no_line_when_anything_is_unsure() {
    for command in [
        // A path that holds no failing test may be an option's value.
        "pytest other",
        "pytest --unknown-option value",
        // An option of unknown kind would take the first id as its value.
        "pytest tests --unknown",
        "pytest tests -k",
        // The same path twice cannot be taken out word by word.
        "pytest tests && pytest tests",
        "pytest -- tests",
        "python -c 'import pytest'",
        // The line already runs exactly the failing tests.
        "pytest tests/test_checkout.py::test_checkout_total \
         tests/test_pricing.py::test_bulk_price",
    ] {
        assert_eq!(rerun(command, PYTEST), None, "{command}");
    }
    for output in [
        // The summary lists fewer than the counts.
        "==== short test summary info ====\n\
         FAILED t/test_a.py::test_x - boom\n\
         ==== 2 failed in 0.10s ====\n",
        // No short summary: `-rN`.
        "==== 1 failed, 3 passed in 0.10s ====\n",
        // A parameter with a space cannot be told from the message.
        "==== short test summary info ====\n\
         FAILED t/test_a.py::test_x[a b] - boom\n\
         ==== 1 failed in 0.10s ====\n",
        "==== 3 passed in 0.10s ====\n",
    ] {
        assert_eq!(rerun("pytest", output), None, "{output}");
    }
}

#[test]
fn npm_test_running_jest_gets_the_failing_files_by_path() {
    for (command, expected) in [
        (
            "npm test",
            "npm test -- --runTestsByPath src/checkout.test.ts",
        ),
        (
            "npm test -- --watchAll=false",
            "npm test -- --watchAll=false --runTestsByPath src/checkout.test.ts",
        ),
        (
            "cd web && npm run test -- --ci",
            "cd web && npm run test -- --ci --runTestsByPath src/checkout.test.ts",
        ),
    ] {
        assert_eq!(narrow(command, NPM_TEST).as_deref(), Some(expected));
    }
}

#[test]
fn npm_test_gets_no_line_unless_the_script_is_plain_jest() {
    for command in [
        "npm test -- src/cart",
        "npm test -- --watch",
        "npm test -- --shard=1/2",
        "npm test -- --onlyFailures",
        "npm run build",
        "pnpm test",
        "yarn test",
    ] {
        assert_eq!(rerun(command, NPM_TEST), None, "{command}");
    }
    let vitest = NPM_TEST.replace("> jest", "> vitest run");
    let jest_with_paths = NPM_TEST.replace("> jest", "> jest src");
    let more_failed = NPM_TEST.replace("Test Suites: 1 failed", "Test Suites: 2 failed");
    for output in [&vitest, &jest_with_paths, &more_failed] {
        assert_eq!(rerun("npm test", output), None, "{output}");
    }
}

/// Untrusted output of any shape gives a line or none, never a panic, and a line always
/// reads back as the command it shows.
#[test]
fn any_output_gives_a_line_that_reads_or_none() {
    let noise = [
        "test  ... FAILED",
        "test result: FAILED. x failed;",
        "test result: FAILED. 99999999999999999999999 failed;",
        "FAILED ",
        "ERROR - x",
        " short test summary info ",
        "== in s ==",
        "Test Suites: failed",
        "FAIL ",
        "> @ test\n> jest",
        "\u{0}\u{fffd}'\"`$(",
    ];
    for command in ["cargo test", "pytest", "npm test"] {
        for line in noise {
            let output = format!("{line}\n{line}\n");
            if let Some(narrow) = narrow(command, &output) {
                words(&narrow);
            }
        }
    }
}
