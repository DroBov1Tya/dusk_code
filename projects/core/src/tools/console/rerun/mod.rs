//! The narrow command after a failed test run (`limits.rerun_hint`): one line that names
//! the command re-running only the tests that failed, so the model iterates on those and
//! not on the whole suite. It is derived from the command line and the output alone, and
//! only when both leave no doubt: the program is known, its own test filters are not in
//! the way, every failing test is named and the names add up to the count the runner
//! printed. Otherwise there is no line.

mod cargo;
mod jest;
mod pytest;
#[cfg(test)]
mod tests;

use super::command_line::{Reading, run_words, split};
use super::shape::program::{Program, program};

/// More failing tests than this make a line too long to be a narrow command.
const MAX_TESTS: usize = 10;

/// The line to add after the output of `command`, which failed; `None` when the narrow
/// command is not certain.
pub fn rerun(command: &str, output: &str) -> Option<String> {
    if command.contains('\n') {
        return None;
    }
    let narrow = match program(command) {
        Program::Cargo { tests: true } => cargo::narrow(command, output),
        Program::Pytest => pytest::narrow(command, output),
        Program::Npm => jest::narrow(command, output),
        _ => None,
    }?;
    Some(format!("Re-run only the failing tests: `{narrow}`"))
}

/// The words of the command `line` runs, as `program` reads it: the last of its commands,
/// after any `cd` steps.
fn last_words(line: &str) -> Option<Vec<String>> {
    split(line, Reading::Strict)?.commands.pop()
}

/// `line` with `words` added at its end, each quoted for `sh`, when the line still reads
/// as the same commands with exactly those words after the last.
fn appended(line: &str, words: &[String]) -> Option<String> {
    rewritten(line, &[], words)
}

/// `line` without the words `removed` and with `added` at its end, when the line still
/// reads as the same commands with exactly that change to the last. A removed word must
/// stand alone and once in the line, between spaces, so taking it out cannot touch
/// another word.
fn rewritten(line: &str, removed: &[&str], added: &[String]) -> Option<String> {
    let mut text = line.trim_end().to_owned();
    let mut expected = last_words(line)?;
    for word in removed {
        let pieces: Vec<&str> = text.split(' ').collect();
        let mut at = pieces
            .iter()
            .enumerate()
            .filter(|(_, piece)| piece == &word);
        let (Some((index, _)), None) = (at.next(), at.next()) else {
            return None;
        };
        let rest: Vec<&str> = (pieces.iter().enumerate())
            .filter(|(i, _)| *i != index)
            .map(|(_, piece)| *piece)
            .collect();
        text = rest.join(" ");
        let place = expected.iter().position(|seen| seen == word)?;
        expected.remove(place);
    }
    for word in added {
        text.push(' ');
        text.push_str(&quoted(word));
    }
    expected.extend(added.iter().cloned());
    reads_as(&text, line, &expected).then_some(text)
}

/// Whether `text` splits into the commands of `line` with its last command's words
/// `expected`, and still runs the same program.
fn reads_as(text: &str, line: &str, expected: &[String]) -> bool {
    let (Some(new), Some(old)) = (split(text, Reading::Strict), split(line, Reading::Strict))
    else {
        return false;
    };
    let before = old.commands.len().saturating_sub(1);
    new.only_and == old.only_and
        && new.commands.len() == old.commands.len()
        && new.commands.get(..before) == old.commands.get(..before)
        && new.commands.last().map(Vec::as_slice) == Some(expected)
        && program(text) == program(line)
}

/// Whether `word` holds only characters no shell treats specially.
fn plain(word: &str) -> bool {
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./:=,+@%".contains(c))
}

/// `word` as `sh` reads it back: bare when plain, single-quoted otherwise.
fn quoted(word: &str) -> String {
    if plain(word) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// `names` with repeats dropped, first seen first; `None` when there are none or too many.
fn distinct(names: Vec<String>) -> Option<Vec<String>> {
    let mut seen: Vec<String> = Vec::new();
    for name in names {
        if !seen.contains(&name) {
            seen.push(name);
        }
    }
    (!seen.is_empty() && seen.len() <= MAX_TESTS).then_some(seen)
}

/// `12` from a count word.
fn number(word: &str) -> Option<usize> {
    (!word.is_empty() && word.bytes().all(|b| b.is_ascii_digit()))
        .then(|| word.parse().ok())
        .flatten()
}
