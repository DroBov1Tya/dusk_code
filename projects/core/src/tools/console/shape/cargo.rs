//! `cargo` building, checking, linting and testing. Build progress and passing tests go;
//! a warning printed again word for word folds into the first, which says how many there
//! were. Every rule matches a whole line of a fixed form, so a diagnostic, a failed test,
//! a summary or a line of the program's own never matches one.

use std::collections::{HashMap, hash_map::Entry};

use super::Kept;

const BUILD_PROGRESS: &str = "build progress";
const PASSING_TESTS: &str = "passing tests";
const REPEATED_WARNINGS: &str = "repeated warnings";

/// Status verbs cargo prints before a crate and its version while it works.
const CRATE_VERBS: [&str; 5] = [
    "Compiling",
    "Checking",
    "Documenting",
    "Fresh",
    "Downloaded",
];

pub(super) fn shape<'a>(lines: &[&'a str], tests: bool, kept: &mut Kept<'a>) {
    // Each warning by its lines: where its first copy is kept, and how many came.
    let mut warnings: HashMap<&[&str], (usize, usize)> = HashMap::new();
    let mut at = 0;
    while let Some(line) = lines.get(at) {
        if !is_warning(line) {
            match left_out(line, tests) {
                Some(what) => kept.leave_out(what, 1),
                None => kept.keep(line),
            }
            at += 1;
            continue;
        }
        let end = diagnostic_end(lines, at);
        let block = lines.get(at..end).unwrap_or_default();
        match warnings.entry(block) {
            Entry::Occupied(mut seen) => {
                seen.get_mut().1 += 1;
                // The blank line after a copy goes with it.
                let blank = lines.get(end).is_some_and(|line| line.is_empty());
                kept.leave_out(REPEATED_WARNINGS, block.len() + usize::from(blank));
                at = end + usize::from(blank);
                continue;
            }
            Entry::Vacant(first) => {
                first.insert((kept.lines.len(), 1));
                for line in block {
                    kept.keep(line);
                }
            }
        }
        at = end;
    }
    for (header, count) in warnings.into_values() {
        if let Some(line) = kept.lines.get_mut(header) {
            line.count = count;
        }
    }
}

fn is_warning(line: &str) -> bool {
    line.starts_with("warning:") || line.starts_with("warning[")
}

/// Where the diagnostic that starts at `start` ends: before the first blank line or the
/// first line that is not part of its frame.
fn diagnostic_end(lines: &[&str], start: usize) -> usize {
    let after = lines.iter().skip(start + 1);
    start + 1 + after.take_while(|line| in_frame(line)).count()
}

/// A line of a diagnostic's frame: a numbered source line, a note or help that belongs
/// to it, or an indented line other than cargo's own status, which follows a warning
/// with no blank line between.
fn in_frame(line: &str) -> bool {
    if ["help:", "note:", "...", "|", "="]
        .iter()
        .any(|start| line.starts_with(start))
    {
        return true;
    }
    let numbered = line.trim_start_matches(|c: char| c.is_ascii_digit());
    if numbered.len() < line.len() {
        return numbered.trim_start().starts_with(['|', '+', '-', '~']);
    }
    let text = line.trim_start();
    text.len() < line.len() && !text.is_empty() && !is_status(text)
}

/// `Finished ...`, `Compiling ...`, `Doc-tests ...`: a capitalized verb, as cargo
/// right-aligns them.
fn is_status(text: &str) -> bool {
    let Some((verb, _)) = text.split_once(' ') else {
        return false;
    };
    let mut chars = verb.chars();
    chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.all(|c| c.is_ascii_lowercase() || c == '-')
}

fn left_out(line: &str, tests: bool) -> Option<&'static str> {
    if is_progress(line.trim_start()) {
        return Some(BUILD_PROGRESS);
    }
    (tests && is_passing_test(line)).then_some(PASSING_TESTS)
}

/// `Compiling serde v1.0.228`, `Checking core v0.1.0 (/path)`, `Updating crates.io
/// index`, `Blocking waiting for file lock on build directory`.
fn is_progress(line: &str) -> bool {
    let Some((verb, rest)) = line.split_once(' ') else {
        return false;
    };
    match verb {
        verb if CRATE_VERBS.contains(&verb) => names_a_crate(rest) || counts_crates(rest),
        "Downloading" => rest.starts_with("crates") || counts_crates(rest),
        "Updating" => rest.ends_with(" index") || rest.starts_with("git repository `"),
        "Blocking" => rest.starts_with("waiting for file lock on "),
        _ => false,
    }
}

/// `serde v1.0.228`, maybe followed by `(where it comes from)`.
fn names_a_crate(rest: &str) -> bool {
    let mut words = rest.splitn(3, ' ');
    let name = words.next().unwrap_or_default();
    let version = words.next().unwrap_or_default();
    let source = words.next();
    let named = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let versioned = version
        .strip_prefix('v')
        .is_some_and(|number| number.starts_with(|c: char| c.is_ascii_digit()));
    named
        && versioned
        && source.is_none_or(|source| source.starts_with('(') && source.ends_with(')'))
}

/// `3 crates (1.2MB) in 0.5s`, `1 crate`.
fn counts_crates(rest: &str) -> bool {
    let mut words = rest.split(' ');
    let number = words.next().unwrap_or_default();
    let noun = words.next().unwrap_or_default();
    !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit())
        && (noun == "crate" || noun == "crates")
}

/// `test tests::adds ... ok`: the whole line, with the test's name and nothing else.
fn is_passing_test(line: &str) -> bool {
    line.strip_prefix("test ")
        .and_then(|rest| rest.strip_suffix(" ... ok"))
        .is_some_and(|name| !name.is_empty() && !name.contains(" ... "))
}
