//! Which program a command line runs, read from the line as `sh` would split it. Only a
//! plain command, or one after `cd` steps joined by `&&`, names a program: behind a pipe,
//! a list or a background job the output is not that program's alone.

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
    /// Output that is content the model may act on word for word, such as a diff or a
    /// file, or a line this reading cannot follow: left as it is.
    Verbatim,
    /// Anything else: only repeated lines fold.
    Other,
}

/// Commands whose output is content, not a report about work.
const VERBATIM: [&str; 6] = ["cat", "head", "tail", "sed", "nl", "diff"];

/// The program `line` runs. A line with a substitution or a quote left open is not
/// followed, so it is `Verbatim`.
pub fn program(line: &str) -> Program {
    let Some(split) = split(line) else {
        return Program::Verbatim;
    };
    let programs: Vec<Program> = split.commands.iter().map(|words| of(words)).collect();
    if programs.contains(&Program::Verbatim) {
        return Program::Verbatim;
    }
    match split.commands.split_last() {
        Some((last, before)) if split.only_and && before.iter().all(|words| is_cd(words)) => {
            of(last)
        }
        _ => Program::Other,
    }
}

/// A line's commands, each as its words, and whether only `&&` joined them.
struct Split {
    commands: Vec<Vec<String>>,
    only_and: bool,
}

/// The words of each command of `line`, quotes taken off. `None` for a substitution, a
/// subshell or a quote that does not close.
fn split(line: &str) -> Option<Split> {
    let mut split = Split {
        commands: vec![Vec::new()],
        only_and: true,
    };
    let mut word = String::new();
    // A word can be empty yet present, as `''` is.
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => word.push(c),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '`' => return None,
                        '$' if chars.peek() == Some(&'(') => return None,
                        '\\' => word.push(chars.next()?),
                        c => word.push(c),
                    }
                }
            }
            '\\' => match chars.next() {
                // A line continuation joins the lines.
                Some('\n') | None => {}
                Some(c) => {
                    in_word = true;
                    word.push(c);
                }
            },
            '`' | '(' | ')' => return None,
            '$' if chars.peek() == Some(&'(') => return None,
            '#' if !in_word => while chars.next_if(|&c| c != '\n').is_some() {},
            '&' if chars.peek() == Some(&'&') => {
                chars.next();
                split.end_command(&mut word, &mut in_word, true);
            }
            // `2>&1` and `&>file` redirect; a lone `&` sends a job to the background.
            '&' if word.ends_with('>') || chars.peek() == Some(&'>') => {
                in_word = true;
                word.push(c);
            }
            '&' | ';' | '\n' => split.end_command(&mut word, &mut in_word, false),
            '|' => {
                chars.next_if(|&c| c == '|' || c == '&');
                split.end_command(&mut word, &mut in_word, false);
            }
            c if c.is_whitespace() => split.end_word(&mut word, &mut in_word),
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    split.end_word(&mut word, &mut in_word);
    split.commands.retain(|words| !words.is_empty());
    Some(split)
}

impl Split {
    fn end_word(&mut self, word: &mut String, in_word: &mut bool) {
        if std::mem::take(in_word)
            && let Some(command) = self.commands.last_mut()
        {
            command.push(std::mem::take(word));
        }
    }

    fn end_command(&mut self, word: &mut String, in_word: &mut bool, and: bool) {
        self.end_word(word, in_word);
        self.only_and &= and;
        self.commands.push(Vec::new());
    }
}

fn is_cd(words: &[String]) -> bool {
    words.first().is_some_and(|word| word == "cd")
}

/// The program of one command: what runs after its variable assignments, with its
/// redirections left out.
fn of(words: &[String]) -> Program {
    let words: Vec<&str> = words
        .iter()
        .map(String::as_str)
        .skip_while(|word| is_assignment(word))
        .filter(|word| !is_redirection(word))
        .collect();
    of_words(&words)
}

fn of_words(words: &[&str]) -> Program {
    let Some((first, rest)) = words.split_first() else {
        return Program::Other;
    };
    let name = first.rsplit('/').next().unwrap_or(first);
    match name {
        "cargo" => cargo(rest),
        "pytest" | "py.test" => Program::Pytest,
        "npm" | "pnpm" | "yarn" => npm(name, rest),
        "git" => git(rest),
        "uv" => match rest {
            ["run", program @ ..] => of_words(program),
            _ => Program::Other,
        },
        python if is_python(python) => match rest {
            ["-m", "pytest", ..] => Program::Pytest,
            _ => Program::Other,
        },
        name if VERBATIM.contains(&name) => Program::Verbatim,
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
            _ => return Program::Verbatim,
        }
    }
    Program::Verbatim
}

fn is_python(name: &str) -> bool {
    name == "python" || name == "python3" || name.starts_with("python3.")
}

/// `NAME=value`, as before a command.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        let mut chars = name.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// `>file`, `2>&1`, `<input`, `&>log`.
fn is_redirection(word: &str) -> bool {
    word.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&')
        .starts_with(['>', '<'])
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
        ] {
            assert_eq!(program(line), expected, "{line}");
        }
    }

    /// Chained or piped, the output is not one program's: only repeats fold. Content,
    /// and what cannot be followed, stays whole.
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
            "npx tsc --noEmit",
            "npm view react",
            "ls -la",
            "",
        ] {
            assert_eq!(program(line), Program::Other, "{line}");
        }
        for line in [
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
        ] {
            assert_eq!(program(line), Program::Verbatim, "{line}");
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
