//! When two failures are the same: real-looking pairs that must match and pairs that
//! must not, the program a command runs, and the normalizer on hostile text.

use proptest::prelude::*;
use serde_json::json;

use super::signature::{self, normalize, program};

fn key(command: &str, output: &str) -> String {
    let arguments = json!({ "command": command });
    signature::of("run_command", &arguments, output)
        .expect("an error to recognize")
        .key
}

const MISSING_X: &str = "exit code: 101
stderr:
   Compiling app v0.1.0 (/work/app)
error[E0425]: cannot find value `x` in this scope
 --> src/main.rs:LINE:13
  |
LINE |     let y = x;
  |             ^ not found in this scope

For more information about this error, try `rustc --explain E0425`.
error: could not compile `app` (bin \"app\") due to 1 previous error";

const FAILED_TEST: &str = "exit code: 101
stdout:
running 3 tests
test tests::parses ... ok
test tests::adds ... FAILED

failures:

---- tests::adds stdout ----
thread 'tests::adds' panicked at src/lib.rs:10:9:
assertion `left == right` failed
  left: 4
 right: 5

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in DURATION

error: test failed, to rerun pass `--lib`";

#[test]
fn the_same_error_through_varied_calls_has_one_signature() {
    let same = [
        (
            "the same compiler error at two lines",
            ("cargo build", MISSING_X.replace("LINE", "2")),
            (
                "cd app && cargo build --quiet",
                MISSING_X.replace("LINE", "7"),
            ),
        ),
        (
            "the same missing file at two paths",
            (
                "cat /tmp/a/config.yml",
                "exit code: 1\nstderr:\ncat: /tmp/a/config.yml: No such file or directory".into(),
            ),
            (
                "sudo cat ./conf/config.yml",
                "exit code: 1\nstderr:\ncat: ./conf/config.yml: No such file or directory".into(),
            ),
        ),
        (
            "the same failing test at two durations",
            ("cargo test", FAILED_TEST.replace("DURATION", "0.00s")),
            (
                "RUST_BACKTRACE=1 cargo test --lib",
                FAILED_TEST.replace("DURATION", "1.27s"),
            ),
        ),
        (
            "the same crash at two addresses and times",
            (
                "./server --port 8080",
                "exit code: 134\nstderr:\n2026-11-17T10:01:02Z fatal: segfault at 0x7ffd5e8a1c20 \
                 after 950ms"
                    .into(),
            ),
            (
                "timeout 60 /opt/bin/server --port 9090",
                "exit code: 134\nstderr:\n2026-11-18T23:59:00Z fatal: segfault at 0x55d1aa00 \
                 after 3s"
                    .into(),
            ),
        ),
        (
            "the same python error through two paths of one program",
            (
                "python3 app.py",
                "exit code: 1\nstderr:\nTraceback (most recent call last):\n  File \
                 \"/work/app.py\", line 3, in <module>\nModuleNotFoundError: No module named \
                 'requests'"
                    .into(),
            ),
            (
                "PYTHONPATH=. /usr/bin/python3 ./src/app.py",
                "exit code: 1\nstderr:\nTraceback (most recent call last):\n  File \
                 \"/work/src/app.py\", line 12, in <module>\nModuleNotFoundError: No module \
                 named 'requests'"
                    .into(),
            ),
        ),
    ];
    for (what, (first, first_output), (second, second_output)) in same {
        assert_eq!(
            key(first, &first_output),
            key(second, &second_output),
            "{what}"
        );
    }
}

#[test]
fn different_problems_have_different_signatures() {
    let different = [
        (
            "different error kinds",
            ("cargo build", MISSING_X.replace("LINE", "2")),
            (
                "cargo build",
                "error[E0308]: mismatched types\n --> src/main.rs:2:13\nerror: could not \
                 compile `app` (bin \"app\") due to 1 previous error"
                    .into(),
            ),
        ),
        // A judgement call: a different missing name is usually progress, such as the
        // next dependency after one was installed, so it is not the same failure.
        (
            "different missing modules",
            (
                "python3 app.py",
                "ModuleNotFoundError: No module named 'requests'".into(),
            ),
            (
                "python3 app.py",
                "ModuleNotFoundError: No module named 'numpy'".into(),
            ),
        ),
        (
            "different failing tests",
            ("cargo test", FAILED_TEST.replace("DURATION", "0.00s")),
            (
                "cargo test",
                FAILED_TEST
                    .replace("DURATION", "0.00s")
                    .replace("tests::adds", "tests::subtracts"),
            ),
        ),
        (
            "different programs with the same error",
            ("make", "error: missing operand".into()),
            ("cmake", "error: missing operand".into()),
        ),
    ];
    for (what, (first, first_output), (second, second_output)) in different {
        assert_ne!(
            key(first, &first_output),
            key(second, &second_output),
            "{what}"
        );
    }
}

#[test]
fn another_tool_is_known_by_its_name_and_error() {
    let read = |path: &str| {
        let error = format!("error: cannot read {path}: No such file or directory");
        signature::of("read_file", &json!({ "path": path }), &error).map(|s| s.key)
    };
    assert_eq!(read("/a/b.txt"), read("/c/d/e.txt"));
    assert!(read("/a/b.txt").unwrap().starts_with("read_file\n"));
}

#[test]
fn a_failure_with_nothing_to_read_has_no_signature() {
    let arguments = json!({ "command": "grep -r needle ." });
    assert!(signature::of("run_command", &arguments, "exit code: 1").is_none());
    assert!(signature::of("run_command", &arguments, "").is_none());
}

#[test]
fn the_excerpt_is_the_error_as_written() {
    let output = MISSING_X.replace("LINE", "2");
    let found = signature::of("run_command", &json!({ "command": "cargo build" }), &output);
    let excerpt = found.unwrap().excerpt;
    assert!(
        excerpt.contains("cannot find value `x` in this scope"),
        "{excerpt}"
    );
    assert!(excerpt.ends_with("due to 1 previous error"), "{excerpt}");
}

#[test]
fn the_program_is_what_runs_after_setup_and_wrappers() {
    let cases = [
        ("cargo test", Some("cargo")),
        ("cd app && cargo test 2>&1 | tail -20", Some("cargo")),
        ("cd /x; export A=1; npm run build", Some("npm")),
        (
            "set -e\nsource .venv/bin/activate && pytest -q",
            Some("pytest"),
        ),
        ("RUST_LOG=debug CI=1 cargo run", Some("cargo")),
        ("sudo -E env FOO=1 /usr/bin/make install", Some("make")),
        (
            "timeout 60s nice -n 10 ./scripts/check.sh",
            Some("check.sh"),
        ),
        ("(cd web && npm test)", Some("npm")),
        ("cd /nowhere", None),
        ("", None),
        ("   ", None),
    ];
    for (command, expected) in cases {
        assert_eq!(program(command).as_deref(), expected, "{command:?}");
    }
}

#[test]
fn the_normalizer_makes_incidental_details_alike() {
    let cases = [
        ("Error at  src/a.rs:12:5", "error at <path>:#:#"),
        (
            "FAILED tests/test_x.py::test_add - assert 1 == 2",
            "failed <path>::test_add - assert # == #",
        ),
        (
            "can't open file 'C:\\work\\app.py'",
            "can't open file '<q>'",
        ),
        (
            "expected `Vec<String>`, found `&str`",
            "expected `vec<string>`, found `&str`",
        ),
        (
            "error: \"a long sentence in quotes\" here",
            "error: \"<q>\" here",
        ),
        (
            "finished in 0.53s; took 950ms; v1.2.3",
            "finished in #; took #; v#",
        ),
        ("python3.11 at 0xdeadbeef id 3f2a9c1d", "python# at # id #"),
        ("it's an unclosed 'quote", "it's an unclosed 'quote"),
        ("2026-11-17T10:01:02Z", "#-#-#t#:#:#z"),
    ];
    for (line, expected) in cases {
        assert_eq!(normalize(line), expected, "{line:?}");
    }
}

#[test]
fn hostile_output_is_read_in_bounded_time_without_a_panic() {
    let huge = "'".repeat(200_000);
    let deep = "`a ".repeat(100_000);
    let multibyte = "é".repeat(10_000) + "\nerror: ünïcödé ‘x’ “y z”";
    let one_line = "x/".repeat(100_000) + " error";
    for output in [
        huge,
        deep,
        multibyte,
        one_line,
        "\0\u{feff}\r\n\r\n".repeat(1000),
    ] {
        let found = signature::of("run_command", &json!({ "command": output }), &output);
        if let Some(found) = found {
            assert!(found.key.len() < 3 * 4096, "the key is bounded");
            assert!(
                found.excerpt.chars().count() <= 401,
                "the excerpt is bounded"
            );
        }
    }
}

proptest! {
    #[test]
    fn any_text_is_normalized_without_a_panic(text in any::<String>(), command in any::<String>()) {
        let normalized = normalize(&text);
        prop_assert!(!normalized.contains("  "));
        let _ = program(&command);
        let _ = signature::of("run_command", &json!({ "command": command }), &text);
    }

    #[test]
    fn digits_never_tell_two_lines_apart(prefix in "[a-z ]{0,20}", a in 0u64..1_000_000, b in 0u64..1_000_000) {
        prop_assert_eq!(
            normalize(&format!("{prefix} error at line {a}")),
            normalize(&format!("{prefix} error at line {b}"))
        );
    }
}
