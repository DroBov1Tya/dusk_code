//! Shaping on output captured from the programs, in the form `run_command` returns it:
//! what goes, what must stay, and that the same input always gives the same text.

use proptest::prelude::*;

use super::{Shaped, shape};

const CARGO_TEST: &str = include_str!("fixtures/cargo_test.txt");
const CARGO_CLIPPY: &str = include_str!("fixtures/cargo_clippy.txt");
const PYTEST: &str = include_str!("fixtures/pytest.txt");
const PYTEST_VERBOSE: &str = include_str!("fixtures/pytest_verbose.txt");
const NPM_INSTALL: &str = include_str!("fixtures/npm_install.txt");
const NPM_TEST: &str = include_str!("fixtures/npm_test.txt");
const GIT_PUSH: &str = include_str!("fixtures/git_push.txt");

/// Each fixture with the command that printed it.
const FIXTURES: [(&str, &str); 7] = [
    ("cargo +1.99.1 test", CARGO_TEST),
    ("cargo clippy --all-targets", CARGO_CLIPPY),
    ("python -m pytest", PYTEST),
    ("pytest -v", PYTEST_VERBOSE),
    ("npm install", NPM_INSTALL),
    ("npm test", NPM_TEST),
    ("git push origin main", GIT_PUSH),
];

fn shaped(command: &str, output: &str) -> Shaped {
    shape(command, output).unwrap_or_else(|| panic!("{command} was not shaped"))
}

/// A kept line as it was, a fold's count taken off.
fn unfolded(line: &str) -> &str {
    match line.rsplit_once(" (x") {
        Some((line, count)) if count.trim_end_matches(')').parse::<usize>().is_ok() => line,
        _ => line,
    }
}

/// Every line of `output` that names an error, a failure, a warning or a place in a file
/// is in `text`, except the ones `expected_gone` lists; and every line of `text` was in
/// `output`.
fn keeps_trouble(output: &str, text: &str, expected_gone: &[&str]) {
    let kept: Vec<&str> = text.split('\n').map(unfolded).collect();
    for line in output.split('\n') {
        if super::troubled(line) && !expected_gone.contains(&line) {
            assert!(kept.contains(&line), "lost {line:?}\n{text}");
        }
    }
    for line in &kept {
        assert!(output.split('\n').any(|seen| seen == *line), "new {line:?}");
    }
}

fn lines_of(output: &str) -> usize {
    output.split('\n').count()
}

#[test]
fn cargo_test_keeps_failures_and_summaries_and_drops_passes_and_progress() {
    let shaped = shaped("cargo +1.99.1 test", CARGO_TEST);
    let text = &shaped.text;
    // Crate names and test names are not trouble: these lines are progress and passes.
    let names = [
        "   Compiling thiserror v2.0.21",
        "   Compiling thiserror-impl v2.0.21",
        "test tests::error_code_parses ... ok",
    ];
    keeps_trouble(CARGO_TEST, text, &names);
    assert!(
        !text.contains("Compiling") && !text.contains(" ... ok"),
        "{text}"
    );
    for kept in [
        "exit code: 101",
        "running 67 tests",
        "test tests::slow ... ignored",
        "test tests::another_failure ... FAILED",
        "---- tests::fails_on_purpose stdout ----",
        "assertion `left == right` failed: math is broken",
        "  left: 4",
        "test result: FAILED. 64 passed; 2 failed; 1 ignored; 0 measured; 0 filtered out; \
         finished in 0.00s",
        "    Finished `test` profile [unoptimized + debuginfo] target(s) in 2.67s",
        "error: test failed, to rerun pass `--lib`",
        "      Adding anyhow v1.0.104",
        "     Locking 10 packages to highest compatible versions",
    ] {
        assert!(
            text.split('\n').any(|line| line == kept),
            "{kept:?}\n{text}"
        );
    }
    // stdout comes first in a result, and the tests are in it.
    assert_eq!(shaped.what, "passing tests, build progress");
    assert_eq!(shaped.left_out, 11 + 64);
    assert_eq!(lines_of(text) + shaped.left_out, lines_of(CARGO_TEST));
}

#[test]
fn cargo_clippy_keeps_every_diagnostic() {
    let shaped = shaped("cargo clippy --all-targets", CARGO_CLIPPY);
    keeps_trouble(
        CARGO_CLIPPY,
        &shaped.text,
        &["    Checking thiserror v2.0.21"],
    );
    assert_eq!(
        (shaped.left_out, shaped.what.as_str()),
        (5, "build progress")
    );
    let error = CARGO_CLIPPY
        .split_once("error[E0308]")
        .map(|(_, rest)| rest)
        .unwrap();
    assert!(shaped.text.ends_with(error), "{}", shaped.text);
}

/// A warning printed again word for word folds into the first; one that differs in
/// anything, its place included, stays.
#[test]
fn identical_warnings_fold_into_the_first_with_a_count() {
    let warning = |file: &str| {
        format!(
            "warning: unused variable: `x`\n --> {file}:1:5\n  |\n1 | let x = 1;\n  |     ^\n  |\n  \
             = note: `#[warn(unused_variables)]` on by default\n"
        )
    };
    let output = format!(
        "exit code: 0\nstderr:\n{a}\n{b}\n{a}\n{a}\nwarning: `x` (lib) generated 1 warning\n    \
         Finished `dev` profile in 0.1s\n",
        a = warning("src/a.rs"),
        b = warning("src/b.rs"),
    );
    let shaped = shaped("cargo build", &output);
    assert_eq!(
        shaped.text,
        format!(
            "exit code: 0\nstderr:\nwarning: unused variable: `x` (x3)\n{}\n{}\nwarning: `x` (lib) \
             generated 1 warning\n    Finished `dev` profile in 0.1s\n",
            warning("src/a.rs").trim_start_matches("warning: unused variable: `x`\n"),
            warning("src/b.rs"),
        )
    );
    assert_eq!(
        (shaped.left_out, shaped.what.as_str()),
        (16, "repeated warnings")
    );
}

#[test]
fn pytest_keeps_failures_errors_warnings_and_the_summary() {
    for (command, output, dropped) in [
        ("python -m pytest", PYTEST, 2),
        ("pytest -v", PYTEST_VERBOSE, 38),
    ] {
        let shaped = shaped(command, output);
        keeps_trouble(output, &shaped.text, &[]);
        assert_eq!(shaped.left_out, dropped, "{}", shaped.text);
        assert_eq!(shaped.what, "passing tests");
        assert!(!shaped.text.contains(" PASSED"), "{}", shaped.text);
        // A row with a failure, an error, a skip or an xfail stays.
        assert_eq!(
            shaped.text.matches(".py ...").count(),
            usize::from(command == "python -m pytest") * 2
        );
    }
}

#[test]
fn npm_keeps_warnings_failures_and_the_summary() {
    let install = shaped("npm install", NPM_INSTALL);
    keeps_trouble(NPM_INSTALL, &install.text, &[]);
    assert_eq!(install.what, "install progress and notices");
    assert_eq!(install.left_out, 8);
    for kept in [
        "added 412 packages, and audited 413 packages in 9s",
        "npm warn deprecated glob@7.2.3: Glob versions prior to v9 are no longer supported",
    ] {
        assert!(install.text.contains(kept), "{}", install.text);
    }

    let test = shaped("npm test", NPM_TEST);
    keeps_trouble(NPM_TEST, &test.text, &[]);
    assert_eq!((test.left_out, test.what.as_str()), (46, "passing tests"));
    for kept in [
        "FAIL src/checkout.test.ts",
        "    ✕ adds tax (4 ms)",
        "      at Object.<anonymous> (src/checkout.test.ts:21:25)",
        "Tests:       1 failed, 41 passed, 42 total",
    ] {
        assert!(test.text.contains(kept), "{}", test.text);
    }
}

#[test]
fn git_drops_only_transfer_progress() {
    let shaped = shaped("git push origin main", GIT_PUSH);
    keeps_trouble(GIT_PUSH, &shaped.text, &[]);
    assert_eq!(
        shaped.text,
        "exit code: 0\nstderr:\nTotal 13 (delta 9), reused 0 (delta 0), pack-reused 0\n\
         remote: warning: See https://gh.io/lfs for more information.\n\
         remote: error: GH001: Large files detected. You may want to try Git Large File \
         Storage.\nTo github.com:dev/shop.git\n   3c9247c..8a1f2e0  main -> main\n"
    );
    assert_eq!(shaped.left_out, 6);
}

/// Behind a pipe or a chain the output is not one program's, and a program without
/// rules may print content: none of it changes, not even repeated lines.
#[test]
fn what_is_not_sure_passes_through() {
    for command in ["cargo test | grep test", "make && cargo test", "ls"] {
        assert_eq!(shape(command, CARGO_TEST), None, "{command}");
    }
    let repeats = "exit code: 0\nstdout:\nsame\nsame\nsame\n\n\nother\n";
    for command in [
        "git diff",
        "cat log.txt",
        "cargo test $(x)",
        "cargo t | tail -3",
        "cargo test | grep same",
        "awk '{print $1}' data.txt",
        "python -c 'print(open(\"f\").read())'",
        "sort words.txt",
        "xxd blob.bin",
        "base64 blob.bin",
    ] {
        assert_eq!(shape(command, repeats), None, "{command}");
    }
    // A known program without rules of its own still folds its repeats.
    let folded = shaped("npx tsc --noEmit", repeats);
    assert_eq!(folded.text, "exit code: 0\nstdout:\nsame (x3)\n\n\nother\n");
    assert_eq!(
        (folded.left_out, folded.what.as_str()),
        (2, "repeated lines")
    );
}

/// Two warnings that differ only in their frame are counted apart by cargo's rule; the
/// fold of identical lines must not count their headers again (found by the property
/// test below).
#[test]
fn a_counted_warning_is_not_folded_again() {
    let output = "warning: x\nwarning: x\n  |\nwarning: x\n  |\nwarning: x";
    let shaped = shaped("cargo test", output);
    assert_eq!(shaped.text, "warning: x (x2)\nwarning: x (x2)\n  |");
    assert_eq!(
        (shaped.left_out, shaped.what.as_str()),
        (3, "repeated warnings")
    );
}

/// The text sent is decided once: the same input gives the same text, and shaping it
/// again finds nothing more to leave out.
#[test]
fn shaping_is_deterministic_and_idempotent() {
    for (command, output) in FIXTURES {
        let first = shaped(command, output);
        assert_eq!(shape(command, output), Some(first.clone()));
        assert_eq!(shape(command, &first.text), None, "{command}");
    }
}

proptest! {
    /// Any output of any of these commands is shaped without a panic, and a shaped text
    /// holds only lines the output had.
    #[test]
    fn any_output_is_shaped_safely(
        lines in prop::collection::vec(
            prop_oneof![
                "\\PC{0,30}",
                Just("test a ... ok".to_owned()),
                Just("   Compiling a v1.0.0".to_owned()),
                Just("warning: x".to_owned()),
                Just("  |".to_owned()),
                Just("tests/a.py .... [ 50%]".to_owned()),
                Just("Counting objects: 1%\r2%".to_owned()),
                Just("✓ ok".to_owned()),
                Just(String::new()),
            ],
            0..60,
        ),
        command in prop_oneof![
            Just("cargo test"),
            Just("pytest"),
            Just("npm test"),
            Just("git pull"),
            Just("tsc"),
            Just("ls"),
        ],
    ) {
        let output = lines.join("\n");
        if let Some(shaped) = shape(command, &output) {
            prop_assert!(shaped.left_out > 0);
            // Every line left out leaves an empty text, which has no lines.
            let kept = shaped.text.split('\n').filter(|_| !shaped.text.is_empty());
            for line in kept {
                let line = unfolded(line);
                prop_assert!(output.split('\n').any(|seen| seen == line), "{line:?}");
            }
        }
    }
}
