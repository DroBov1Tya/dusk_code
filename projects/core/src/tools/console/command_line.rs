//! A command line read as `sh` splits it: its commands, each as its words with the quotes
//! taken off, and the program one command runs. Output shaping (`shape::program`) and the
//! stuck-loop signature (`runtime::agent::repeats::signature`) both read lines here; each
//! keeps its own rule for which command of a line counts.
//!
//! Approvals do not use this reading: a yes for the session covers only a line with no
//! shell syntax at all, so they refuse what this would follow. They share `WRAPPERS`.

/// Programs that run the next word as a command: `sudo cargo test` runs cargo. Their
/// options and numbers are skipped; an option's separate value is not known, so `sudo -u
/// me make` reads as `me`, which is only ever alike to itself.
pub(crate) const WRAPPERS: &[&str] = &[
    "sudo", "doas", "env", "time", "nice", "nohup", "command", "builtin", "exec", "timeout",
];

/// How a line `split` cannot follow is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reading {
    /// A substitution, a subshell or a quote that does not close: no reading at all.
    Strict,
    /// A substitution or a subshell ends a command and starts the next; a quote that does
    /// not close runs to the end of the line. Every line has a reading.
    Lenient,
}

/// A line's commands, each as its words, and whether only `&&` joined them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Split {
    pub commands: Vec<Vec<String>>,
    pub only_and: bool,
}

/// The words of each command of `line`, quotes taken off; linear in the line. `None`
/// only in a `Strict` reading.
pub(crate) fn split(line: &str, reading: Reading) -> Option<Split> {
    let strict = reading == Reading::Strict;
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
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None if strict => return None,
                        None => break,
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('`') if strict => return None,
                        Some('$') if strict && chars.peek() == Some(&'(') => return None,
                        Some('\\') => match chars.next() {
                            Some(c) => word.push(c),
                            None if strict => return None,
                            None => break,
                        },
                        Some(c) => word.push(c),
                        None if strict => return None,
                        None => break,
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
            '`' | '(' | ')' if strict => return None,
            '$' if strict && chars.peek() == Some(&'(') => return None,
            '`' | '(' | ')' => split.end_command(&mut word, &mut in_word, false),
            '$' if chars.peek() == Some(&'(') => {
                chars.next();
                split.end_command(&mut word, &mut in_word, false);
            }
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

/// What one command runs: its program word and the words after it. Redirections are left
/// out, and before the program so are variable assignments, `{` and `!`, and wrappers with
/// their options and numbers (`timeout 60s`, `nice -n 10`). `None` when nothing runs, or
/// the program word is empty.
pub(crate) fn run_words(words: &[String]) -> Option<(&str, Vec<&str>)> {
    let mut words = words
        .iter()
        .map(String::as_str)
        .filter(|word| !is_redirection(word));
    let mut wrapped = false;
    let program = loop {
        let word = words.next()?;
        let skipped = is_assignment(word)
            || matches!(word, "{" | "!")
            || (wrapped
                && (word.starts_with('-') || word.starts_with(|c: char| c.is_ascii_digit())));
        if skipped {
            continue;
        }
        if WRAPPERS.contains(&program_name(word)) {
            wrapped = true;
            continue;
        }
        break word;
    };
    (!program.is_empty()).then(|| (program, words.collect()))
}

/// A program word without its directory: `/usr/bin/make` is `make`.
pub(crate) fn program_name(word: &str) -> &str {
    word.rsplit(['/', '\\']).next().unwrap_or(word)
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
    use super::{Reading, run_words, split};

    fn commands(line: &str, reading: Reading) -> Option<Vec<Vec<String>>> {
        split(line, reading).map(|split| split.commands)
    }

    #[test]
    fn separators_inside_quotes_do_not_split() {
        let words = commands("echo \"a;b\" 'c|d' e\\&f; cargo t", Reading::Strict).unwrap();
        assert_eq!(
            words,
            [vec!["echo", "a;b", "c|d", "e&f"], vec!["cargo", "t"]]
        );
    }

    #[test]
    fn a_strict_reading_refuses_what_a_lenient_one_follows() {
        for line in [
            "(cd web && npm test)",
            "cargo test $(echo -p core)",
            "cargo test `echo`",
            "echo \"$(git diff)\"",
            "cargo test 'unclosed",
        ] {
            assert_eq!(split(line, Reading::Strict), None, "{line}");
            assert!(split(line, Reading::Lenient).is_some(), "{line}");
        }
        assert_eq!(
            commands("(cd web && npm test)", Reading::Lenient).unwrap(),
            [vec!["cd", "web"], vec!["npm", "test"]]
        );
        assert_eq!(
            commands("cargo test 'a b", Reading::Lenient).unwrap(),
            [vec!["cargo", "test", "a b"]]
        );
    }

    #[test]
    fn the_program_is_found_past_assignments_wrappers_and_redirections() {
        for (line, program, rest) in [
            ("cargo test", "cargo", vec!["test"]),
            ("A=1 B= cargo check", "cargo", vec!["check"]),
            ("timeout 60s nice -n 10 ./x.sh -v", "./x.sh", vec!["-v"]),
            (
                "sudo -E env FOO=1 /usr/bin/make install",
                "/usr/bin/make",
                vec!["install"],
            ),
            ("cargo build >build.log 2>&1", "cargo", vec!["build"]),
            ("! cargo test FOO=1", "cargo", vec!["test", "FOO=1"]),
        ] {
            let words = &commands(line, Reading::Strict).unwrap()[0];
            assert_eq!(run_words(words), Some((program, rest)), "{line}");
        }
        for line in ["env", "sudo -E", "X=1", "'' cargo test"] {
            let words = &commands(line, Reading::Strict).unwrap()[0];
            assert_eq!(run_words(words), None, "{line}");
        }
    }
}
