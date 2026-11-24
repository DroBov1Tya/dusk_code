//! Command output shaped for the model (`limits.shape_output`). For a program whose
//! output is known, passes and progress go and identical repeats fold; failures, errors,
//! warnings and every line the rules do not know stay as they were. The whole output
//! stays stored: this decides only the text the model is sent, once, from the command
//! line and the output alone, so it never changes afterwards.

mod cargo;
mod git;
mod npm;
pub(super) mod program;
mod pytest;
#[cfg(test)]
mod tests;

use std::borrow::Cow;

use program::Program;

/// Output with lines left out, and what they were.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shaped {
    pub text: String,
    /// Lines of the output the text does not show.
    pub left_out: usize,
    /// What they were, as `build progress, passing tests`.
    pub what: String,
}

/// What lines a rule left out are, as the note names them.
const REPEATED_LINES: &str = "repeated lines";

/// Words that mark a line as trouble: such a line stays whatever a loose rule says.
const TROUBLE: [&str; 7] = [
    "error",
    "fail",
    "warn",
    "panic",
    "fatal",
    "exception",
    "denied",
];

/// `output` of `command` with what the model does not need left out; `None` when
/// nothing is, or when the command is not one whose output may change.
pub fn shape(command: &str, output: &str) -> Option<Shaped> {
    let lines: Vec<&str> = output.split('\n').collect();
    let mut kept = Kept::default();
    match program::program(command) {
        Program::Cargo { tests } => cargo::shape(&lines, tests, &mut kept),
        Program::Pytest => kept.filter(&lines, pytest::left_out),
        Program::Npm => kept.filter(&lines, npm::left_out),
        Program::GitTransfer => kept.filter(&lines, git::left_out),
        Program::Tsc => kept.filter(&lines, |_| None),
        Program::Other => return None,
    }
    kept.fold();
    kept.shaped()
}

/// The lines a program's rules keep, and how many went, by what they were.
#[derive(Default)]
struct Kept<'a> {
    lines: Vec<Line<'a>>,
    /// In the order first left out, so the note reads the same for the same output.
    gone: Vec<(&'static str, usize)>,
}

/// A kept line and how many it stands for: the fold's count, or a rule's.
#[derive(Clone)]
struct Line<'a> {
    text: Cow<'a, str>,
    count: usize,
}

impl Line<'_> {
    fn written(&self) -> Cow<'_, str> {
        if self.count > 1 {
            Cow::Owned(format!("{} (x{})", self.text, self.count))
        } else {
            Cow::Borrowed(&self.text)
        }
    }
}

impl<'a> Kept<'a> {
    fn keep(&mut self, line: &'a str) {
        self.lines.push(Line {
            text: Cow::Borrowed(line),
            count: 1,
        });
    }

    /// Keeps each line `rule` names nothing for.
    fn filter(&mut self, lines: &[&'a str], rule: impl Fn(&str) -> Option<&'static str>) {
        for line in lines {
            match rule(line) {
                Some(what) => self.leave_out(what, 1),
                None => self.keep(line),
            }
        }
    }

    fn leave_out(&mut self, what: &'static str, lines: usize) {
        match self.gone.iter_mut().find(|(seen, _)| *seen == what) {
            Some((_, count)) => *count += lines,
            None => self.gone.push((what, lines)),
        }
    }

    /// Folds each run of identical lines into the first, with how many there were.
    /// Blank lines stay: they only space the output. A line that already stands for
    /// more than itself, as a warning a rule counted, is not folded again: its count
    /// would be counted twice.
    fn fold(&mut self) {
        let mut folded = Vec::with_capacity(self.lines.len());
        let mut repeats = 0;
        let same = |a: &Line, b: &Line| a.count == 1 && b.count == 1 && a.text == b.text;
        for run in self.lines.chunk_by(same) {
            match run {
                [line, rest @ ..] if !rest.is_empty() && !line.text.trim().is_empty() => {
                    folded.push(Line {
                        text: line.text.clone(),
                        count: run.len(),
                    });
                    repeats += rest.len();
                }
                _ => folded.extend(run.iter().cloned()),
            }
        }
        self.lines = folded;
        if repeats > 0 {
            self.leave_out(REPEATED_LINES, repeats);
        }
    }

    fn shaped(self) -> Option<Shaped> {
        let left_out = self.gone.iter().map(|(_, count)| count).sum();
        if left_out == 0 {
            return None;
        }
        let what: Vec<&str> = self.gone.iter().map(|(what, _)| *what).collect();
        let lines: Vec<Cow<str>> = self.lines.iter().map(Line::written).collect();
        Some(Shaped {
            text: lines.join("\n"),
            left_out,
            what: what.join(", "),
        })
    }
}

/// Whether a line reports trouble or points at a place in a file. A rule that matches
/// lines loosely keeps such a line.
fn troubled(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    TROUBLE.iter().any(|word| lower.contains(word)) || has_location(line)
}

/// `src/a.rs:12` or `a.py:3:5`: a colon and a digit after a word with `.` or `/` in it.
fn has_location(line: &str) -> bool {
    let mut path_like = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' | '/' => path_like = true,
            ':' if path_like && chars.peek().is_some_and(char::is_ascii_digit) => return true,
            c if c.is_whitespace() || c == '(' || c == '\'' || c == '"' => path_like = false,
            _ => {}
        }
    }
    false
}
