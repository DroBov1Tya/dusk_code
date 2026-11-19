//! Which program a command line runs, read from the line as `sh` would split it. Only a
//! plain command, or one after `cd` steps joined by `&&`, names a program: behind a pipe,
//! a list or a background job the output is not that program's alone. Wrappers such as
//! `timeout 60` or `env X=1` leave the output the program's.

use super::super::command_line::{Reading, program_name, run_words, split};

/// What a command's output is, as far as the rules for it go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    /// `cargo` building, checking or linting; `tests` when it runs tests too.
    Cargo {
        tests: bool,
    },
    Pytest,
    /// `npm`, `pnpm` or `yarn` installing packages or running a script.
    Npm,
    /// `git` moving objects to or from a remote.
    GitTransfer,
    /// `tsc`: every line is a diagnostic or its frame, so only repeats fold.
    Tsc,
    /// A program we have no rules for, whose output may be content the model copies word
    /// for word (a file, a diff, `awk`'s or `base64`'s output), or a line this reading
    /// cannot follow: left as it is.
    Other,
}

/// The program `line` runs. A line with a substitution or a quote left open is not
/// followed, so it is `Other`.
pub fn program(line: &str) -> Program {
    let Some(split) = split(line, Reading::Strict) else {
        return Program::Other;
    };
    match split.commands.split_last() {
        Some((last, before)) if split.only_and && before.iter().all(|words| is_cd(words)) => {
            of(last)
        }
        _ => Program::Other,
    }
}

fn is_cd(words: &[String]) -> bool {
    words.first().is_some_and(|word| word == "cd")
}

/// The program of one command: what runs after its variable assignments and wrappers,
/// with its redirections left out.
fn of(words: &[String]) -> Program {
    match run_words(words) {
        Some((program, rest)) => of_words(program, &rest),
        None => Program::Other,
    }
}

fn of_words(program: &str, rest: &[&str]) -> Program {
    let name = program_name(program);
    match name {
        "cargo" => cargo(rest),
        "pytest" | "py.test" => Program::Pytest,
        "npm" | "pnpm" | "yarn" => npm(name, rest),
        "git" => git(rest),
        "tsc" => Program::Tsc,
        "npx" => match rest {
            ["tsc", ..] => Program::Tsc,
            _ => Program::Other,
        },
        "uv" => match rest {
            ["run", program, rest @ ..] => of_words(program, rest),
            _ => Program::Other,
        },
        python if is_python(python) => match rest {
            ["-m", "pytest", ..] => Program::Pytest,
            _ => Program::Other,
        },
        _ => Program::Other,
    }
}

/// `cargo [+toolchain] [-flags] <subcommand>`. A flag that takes a value hides the
/// subcommand, which then reads as unknown.
fn cargo(rest: &[&str]) -> Program {
    let subcommand = rest
        .iter()
        .skip_while(|word| word.starts_with('+'))
        .find(|word| !word.starts_with('-'));
    match subcommand.copied() {
        Some("test" | "t" | "bench") => Program::Cargo { tests: true },
        Some("build" | "b" | "check" | "c" | "clippy" | "doc" | "run" | "r") => {
            Program::Cargo { tests: false }
        }
        _ => Program::Other,
    }
}

fn npm(name: &str, rest: &[&str]) -> Program {
    let subcommand = rest.iter().find(|word| !word.starts_with('-')).copied();
    match subcommand {
        None if name == "yarn" => Program::Npm,
        Some("install" | "i" | "ci" | "add" | "test" | "t" | "run" | "run-script") => Program::Npm,
        _ => Program::Other,
    }
}

/// Only transfers are shaped; every other git command shows content and stays whole.
fn git(rest: &[&str]) -> Program {
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        match *word {
            "-C" | "-c" => {
                words.next();
            }
            option if option.starts_with('-') => {}
            "push" | "pull" | "fetch" | "clone" => return Program::GitTransfer,
            _ => return Program::Other,
        }
    }
    Program::Other
}

fn is_python(name: &str) -> bool {
    name == "python" || name == "python3" || name.starts_with("python3.")
}

#[cfg(test)]
mod tests {
    use super::{Program, program};

    #[test]
    fn the_program_is_read_through_toolchains_variables_and_cd() {
        let cargo_tests = Program::Cargo { tests: true };
        for (line, expected) in [
            ("cargo test", cargo_tests),
            ("cargo +1.99.1 test -p core -- --nocapture", cargo_tests),
            ("RUST_LOG=debug FOO='a b' cargo test 2>&1", cargo_tests),
            ("cd projects/core && cargo test", cargo_tests),
            ("cd a && cd 'b c' && cargo t", cargo_tests),
            (
                "/usr/local/bin/cargo clippy --all-targets",
                Program::Cargo { tests: false },
            ),
            ("cargo build # first", Program::Cargo { tests: false }),
            ("python -m pytest -q tests", Program::Pytest),
            ("python3.12 -m pytest", Program::Pytest),
            ("uv run pytest -x", Program::Pytest),
            ("uv run python -m pytest", Program::Pytest),
            ("pytest", Program::Pytest),
            ("npm install", Program::Npm),
            ("pnpm run build", Program::Npm),
            ("yarn", Program::Npm),
            ("git -C repo push origin main", Program::GitTransfer),
            ("git fetch --all", Program::GitTransfer),
            ("tsc --noEmit", Program::Tsc),
            ("npx tsc --noEmit", Program::Tsc),
            ("node_modules/.bin/tsc -p .", Program::Tsc),
            // A wrapper leaves the output the program's.
            ("timeout 60 cargo test", cargo_tests),
            ("env RUST_BACKTRACE=1 cargo test", cargo_tests),
            ("time nice -n 5 pytest -q", Program::Pytest),
            ("cd web && sudo -E npm ci", Program::Npm),
        ] {
            assert_eq!(program(line), expected, "{line}");
        }
    }

    /// Chained or piped, the output is not one program's; content, a program without
    /// rules and what cannot be followed are not either: all are left as they are.
    #[test]
    fn chains_pipes_content_and_the_unreadable_are_not_shaped() {
        for line in [
            "cargo test | grep FAILED",
            "cargo build; cargo test",
            "cargo test || true",
            "cargo build & cargo test",
            "cargo test |& less",
            "make && cargo test",
            "cargo test\necho done",
            "cargo nextest run",
            "cargo --manifest-path x/Cargo.toml test",
            "npx eslint .",
            "npm view react",
            "ls -la",
            "",
            "git diff",
            "git show HEAD",
            "git log -p",
            "git",
            "cargo test 2>&1 | tail -40",
            "cat Cargo.toml && cargo test",
            "cargo test $(echo -p core)",
            "cargo test `echo`",
            "echo \"$(git diff)\"",
            "(cd x && cargo test)",
            "cargo test 'unclosed",
            "sed -n '1,40p' src/main.rs",
            "awk '{print}' data.txt",
            "python -c 'print(1)'",
            "sort words.txt",
            "xxd image.png",
            "base64 key.bin",
            // A wrapper with nothing to run, or one whose output is a screen.
            "env",
            "timeout 60",
            "watch cargo test",
            "xargs cargo test",
            "timeout 60 cargo test | tail",
        ] {
            assert_eq!(program(line), Program::Other, "{line}");
        }
    }

    #[test]
    fn quotes_and_redirections_do_not_hide_the_subcommand() {
        assert_eq!(
            program("cargo test 'a|b' \"c;d\" -- x\\&\\&y"),
            Program::Cargo { tests: true }
        );
        assert_eq!(
            program("cargo build >build.log 2>&1"),
            Program::Cargo { tests: false }
        );
        assert_eq!(
            program("A=1 B= cargo check"),
            Program::Cargo { tests: false }
        );
    }
}
