//! `pytest`: the node ids of the short test summary, which pytest prints relative to the
//! directory it ran in, in place of the paths the line named. A path is replaced only
//! when it holds a failing test, so it cannot be the value of an option we do not know.

use super::super::command_line::program_name;
use super::{distinct, last_words, number, plain, rewritten, run_words};

/// pytest options (and common plugins') whose value is the next word.
const VALUE_OPTIONS: [&str; 26] = [
    "-k",
    "-m",
    "-p",
    "-c",
    "-o",
    "-W",
    "-r",
    "-n",
    "--rootdir",
    "--basetemp",
    "--confcutdir",
    "--maxfail",
    "--tb",
    "--durations",
    "--deselect",
    "--ignore",
    "--ignore-glob",
    "--junitxml",
    "--junit-xml",
    "--dist",
    "--import-mode",
    "--log-level",
    "--override-ini",
    "--cov",
    "--cov-report",
    "--capture",
];

/// Options known to stand alone; the last option before the ids must be one, or carry its
/// value, so it cannot take the first id as its value.
const LONE_OPTIONS: [&str; 16] = [
    "--lf",
    "--last-failed",
    "--ff",
    "--failed-first",
    "--sw",
    "--stepwise",
    "--exitfirst",
    "--verbose",
    "--quiet",
    "--no-header",
    "--showlocals",
    "--pdb",
    "--strict-markers",
    "--disable-warnings",
    "--no-cov",
    "--runxfail",
];

/// Short options that stand alone, as `-x` or together as `-xvs`.
const LONE_SHORT: &str = "xvsqlR";
/// Short options that take a value, which may follow in the same word: `-ra`, `-n4`.
const VALUE_SHORT: &str = "kmpcoWrn";

pub(super) fn narrow(line: &str, output: &str) -> Option<String> {
    let words = last_words(line)?;
    let (program, rest) = run_words(&words)?;
    let arguments = pytest_arguments(program, &rest)?;
    let ids = failed(output)?;
    let mut paths = Vec::new();
    let mut kept = Vec::new();
    let mut at = 0;
    while let Some(word) = arguments.get(at).copied() {
        at += 1;
        if word == "--" {
            return None;
        }
        kept.push(word);
        if VALUE_OPTIONS.contains(&word) {
            kept.push(arguments.get(at).copied()?);
            at += 1;
        } else if !word.starts_with('-') {
            kept.pop();
            paths.push(word);
        }
    }
    let replaced = paths
        .iter()
        .all(|path| plain(path) && ids.iter().any(|id| holds(path, id)));
    if !replaced || kept.last().is_some_and(|last| !alone(last)) {
        return None;
    }
    rewritten(line, &paths, &ids)
}

/// pytest's own arguments: after `pytest`, `python -m pytest` or `uv run pytest`.
fn pytest_arguments<'a>(program: &str, rest: &[&'a str]) -> Option<Vec<&'a str>> {
    match (program_name(program), rest) {
        ("pytest" | "py.test", _) => Some(rest.to_vec()),
        ("uv", ["run", program, rest @ ..]) => pytest_arguments(program, rest),
        (python, ["-m", "pytest", rest @ ..])
            if python == "python" || python == "python3" || python.starts_with("python3.") =>
        {
            Some(rest.to_vec())
        }
        _ => None,
    }
}

/// Whether the path the line named runs the test `id`.
fn holds(path: &str, id: &str) -> bool {
    let path = path.strip_suffix('/').unwrap_or(path);
    path == "."
        || id == path
        || id
            .strip_prefix(path)
            .is_some_and(|rest| rest.starts_with('/') || rest.starts_with("::"))
}

/// Whether `word`, the last word before the ids, cannot take the first id as its value.
fn alone(word: &str) -> bool {
    if !word.starts_with('-') || word.contains('=') || LONE_OPTIONS.contains(&word) {
        return true;
    }
    let Some(short) = word
        .strip_prefix('-')
        .filter(|short| !short.starts_with('-'))
    else {
        return false;
    };
    let mut letters = short.chars();
    let first = letters.next();
    short.chars().all(|c| LONE_SHORT.contains(c))
        || (first.is_some_and(|c| VALUE_SHORT.contains(c)) && letters.next().is_some())
}

/// The ids of the `FAILED` and `ERROR` lines of the short test summary, when their number
/// is the failed and error count of the last line of counts.
fn failed(output: &str) -> Option<Vec<String>> {
    let mut ids = Vec::new();
    let mut in_summary = false;
    let mut counted = None;
    for line in output.lines() {
        if line.contains(" short test summary info ") {
            in_summary = true;
        } else if let Some(count) = failed_count(line) {
            counted = Some(count);
        } else if in_summary
            && let Some(rest) = line
                .strip_prefix("FAILED ")
                .or_else(|| line.strip_prefix("ERROR "))
        {
            ids.push(node_id(rest)?.to_owned());
        }
    }
    (Some(ids.len()) == counted).then_some(())?;
    distinct(ids)
}

/// `tests/a.py::test_x[1]` from `tests/a.py::test_x[1] - AssertionError: ...`. An id with
/// a space in a parameter cannot be told from the message, so it is not followed.
fn node_id(rest: &str) -> Option<&str> {
    let (id, message) = rest.split_once(' ').unwrap_or((rest, ""));
    let test = id.contains("::") || id.ends_with(".py");
    let closed = !id.contains('[') || id.ends_with(']');
    let ends = message.is_empty() || message.starts_with("- ");
    (test && closed && ends && !id.starts_with('-')).then_some(id)
}

/// `2` from `===== 1 failed, 210 passed, 1 error in 2.41s =====` or, quiet, from
/// `1 failed, 38 passed in 0.31s`: the failed and the errors together.
fn failed_count(line: &str) -> Option<usize> {
    let text = line.trim().trim_matches('=').trim();
    let (counts, time) = text.rsplit_once(" in ")?;
    if !time.ends_with('s') || !time.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let mut failed = 0;
    for part in counts.split(", ") {
        let (count, what) = part.split_once(' ')?;
        let count = number(count)?;
        if matches!(what, "failed" | "error" | "errors") {
            failed += count;
        }
    }
    Some(failed)
}
