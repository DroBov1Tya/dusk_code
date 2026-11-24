//! `cargo test`: the failing tests by their full names, `-- --exact` so no other test
//! whose name contains them runs. Only when the line names no test filter of its own,
//! which would be added to ours, and passes the harness only flags that select nothing.

use super::{appended, distinct, last_words, number, run_words};

/// cargo options whose value is the next word; any other option stands alone.
const VALUE_OPTIONS: [&str; 21] = [
    "-p",
    "--package",
    "--exclude",
    "-F",
    "--features",
    "--target",
    "--target-dir",
    "--manifest-path",
    "--bin",
    "--test",
    "--example",
    "--bench",
    "-j",
    "--jobs",
    "--profile",
    "--color",
    "--config",
    "-Z",
    "--message-format",
    "--lockfile-path",
    "--artifact-dir",
];

/// Test harness flags that change how tests run, not which.
const HARNESS_FLAGS: [&str; 7] = [
    "--nocapture",
    "--show-output",
    "-q",
    "--quiet",
    "--include-ignored",
    "--ignored",
    "--exact",
];

pub(super) fn narrow(line: &str, output: &str) -> Option<String> {
    let words = last_words(line)?;
    let (_, rest) = run_words(&words)?;
    let harness = harness_flags(&rest)?;
    let names = failed(output)?;
    let mut added = Vec::new();
    match harness {
        None => added.extend(["--".to_owned(), "--exact".to_owned()]),
        Some(flags) if !flags.contains(&"--exact") => added.push("--exact".to_owned()),
        Some(_) => {}
    }
    added.extend(names);
    appended(line, &added)
}

/// The flags after `--` (`None` when there is no `--`), when the line is `cargo test`
/// with no test name and only harness flags; the outer `None` otherwise.
fn harness_flags<'a>(rest: &[&'a str]) -> Option<Option<Vec<&'a str>>> {
    let mut words = rest
        .iter()
        .copied()
        .skip_while(|word| word.starts_with('+') || word.starts_with('-'));
    if !matches!(words.next(), Some("test" | "t")) {
        return None;
    }
    while let Some(word) = words.next() {
        if word == "--" {
            let flags: Vec<&str> = words.collect();
            let known = flags.iter().all(|flag| {
                HARNESS_FLAGS.contains(flag)
                    || flag.starts_with("--test-threads=")
                    || flag.starts_with("--color=")
            });
            return known.then_some(Some(flags));
        }
        if VALUE_OPTIONS.contains(&word) {
            words.next()?;
        } else if !word.starts_with('-') {
            // A test name: ours would add to it, not narrow it.
            return None;
        }
    }
    Some(None)
}

/// Each `test <name> ... FAILED` line's name, when their number is the sum of the
/// `test result: FAILED.` lines' failed counts.
fn failed(output: &str) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let mut counted = 0;
    for line in output.lines() {
        if let Some(name) = line
            .strip_prefix("test ")
            .and_then(|rest| rest.strip_suffix(" ... FAILED"))
        {
            let name = name.strip_suffix(" - should panic").unwrap_or(name);
            // A doc test's name has spaces and a line number; it is not followed.
            if name.is_empty() || name.contains(char::is_whitespace) || name.starts_with('-') {
                return None;
            }
            names.push(name.to_owned());
        } else if let Some(summary) = line.strip_prefix("test result: FAILED.") {
            counted += failed_count(summary)?;
        }
    }
    (names.len() == counted).then_some(())?;
    distinct(names)
}

/// `2` from ` 64 passed; 2 failed; 1 ignored; ...`.
fn failed_count(summary: &str) -> Option<usize> {
    summary.split(';').find_map(|part| {
        let count = part.trim().strip_suffix(" failed")?;
        number(count)
    })
}
