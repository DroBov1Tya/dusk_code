//! `npm test` whose script is jest, as npm echoes it: the failing test files, passed as
//! paths (`--runTestsByPath`) so each names one file and nothing else. jest's own names
//! for a test are matched as a pattern, which is not certain, so the line stays at files.

use super::super::command_line::program_name;
use super::{appended, distinct, last_words, number, plain, run_words};

/// jest flags that choose which tests run by something other than the paths.
const SELECTING: [&str; 10] = [
    "-o",
    "--onlyChanged",
    "--changedSince",
    "--lastCommit",
    "--findRelatedTests",
    "--listTests",
    "--watch",
    "--watchAll",
    "--testPathPattern",
    "--testPathIgnorePatterns",
];

const RUN_BY_PATH: &str = "--runTestsByPath";

/// File endings of the tests jest runs by default.
const TEST_FILES: [&str; 6] = [".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs"];

pub(super) fn narrow(line: &str, output: &str) -> Option<String> {
    let words = last_words(line)?;
    let (program, rest) = run_words(&words)?;
    if program_name(program) != "npm" {
        return None;
    }
    let passed = passed_flags(&rest)?;
    if !runs_jest(output) {
        return None;
    }
    let mut added = Vec::new();
    match &passed {
        None => added.extend(["--".to_owned(), RUN_BY_PATH.to_owned()]),
        Some(flags) if !flags.contains(&RUN_BY_PATH) => added.push(RUN_BY_PATH.to_owned()),
        Some(_) => {}
    }
    added.extend(failed(output)?);
    appended(line, &added)
}

/// The flags passed to the script after `--` (`None` without `--`), when the line is
/// `npm test` or `npm run test` and passes it only flags that do not select tests.
fn passed_flags<'a>(rest: &[&'a str]) -> Option<Option<Vec<&'a str>>> {
    let mut words = rest
        .iter()
        .copied()
        .skip_while(|word| word.starts_with('-'));
    match words.next()? {
        "test" | "t" => {}
        "run" | "run-script" if words.next() == Some("test") => {}
        _ => return None,
    }
    while let Some(word) = words.next() {
        if word == "--" {
            let flags: Vec<&str> = words.collect();
            return flags
                .iter()
                .all(|flag| harmless(flag))
                .then_some(Some(flags));
        }
        if !word.starts_with('-') {
            return None;
        }
    }
    Some(None)
}

/// A flag with no separate value that does not choose tests.
fn harmless(flag: &str) -> bool {
    let name = flag.split_once('=').map_or(flag, |(name, _)| name);
    flag.starts_with('-') && !SELECTING.contains(&name)
}

/// Whether npm echoed a `test` script that is jest with flags only.
fn runs_jest(output: &str) -> bool {
    let mut echoed = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .skip_while(|line| !(line.starts_with("> ") && line.ends_with(" test")));
    echoed.next().is_some_and(|line| line.contains('@'))
        && echoed.next().is_some_and(|script| {
            let mut words = script.split_whitespace();
            words.next() == Some(">")
                && words.next() == Some("jest")
                && words.all(|word| harmless(word) && word != RUN_BY_PATH)
        })
}

/// Each `FAIL <file>` line's file, when their number is the failed count of the `Test
/// Suites:` line.
fn failed(output: &str) -> Option<Vec<String>> {
    let mut files = Vec::new();
    let mut counted = None;
    for line in output.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("FAIL ") {
            files.push(test_file(rest)?.to_owned());
        } else if let Some(counts) = line.strip_prefix("Test Suites:") {
            let failed = counts
                .split(',')
                .find_map(|part| part.trim().strip_suffix(" failed"));
            counted = Some(failed.and_then(number)?);
        }
    }
    let files = distinct(files)?;
    (Some(files.len()) == counted).then_some(files)
}

/// `src/a.test.ts` from `src/a.test.ts` or, for a slow file, `src/a.test.ts (5.2 s)`.
fn test_file(rest: &str) -> Option<&str> {
    let (file, time) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
    let timed = time.is_empty() || (time.starts_with('(') && time.ends_with(" s)"));
    let test = TEST_FILES.iter().any(|end| file.ends_with(end));
    (timed && test && plain(file) && !file.starts_with('-')).then_some(file)
}
