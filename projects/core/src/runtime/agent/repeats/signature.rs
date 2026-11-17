//! A failure's signature: what stays the same when a stuck model edits its call a little
//! and gets the same error back. It is the tool, the program a command runs, and the
//! last error lines of the output with incidental details made alike:
//!
//! - lower case, whitespace collapsed;
//! - a quoted text with a space or a path separator in it, or longer than a name, is
//!   `<q>`; a quoted single word stays, since a different name (a module, a value) is a
//!   different problem, such as the next missing dependency after one was installed;
//! - a word with `/` or `\` is a path: each of its `:`-parts with a separator is `<path>`,
//!   so `src/main.rs:12:5` is `<path>:#:#` and a test id after `::` stays;
//! - a word that is a number (digits, a version, an address, a duration, a date-time) is
//!   `#`, and so is a hex id or a uuid; other digits in a word with a letter or `_` stay,
//!   since `test_1` and `test_2` are different things.
//!
//! Tool output is untrusted: only its last few KiB are read, every pass is linear, and
//! nothing here panics on any text.

use serde_json::Value;

/// Errors are at the end of the output, and the cap bounds the work.
const TAIL_BYTES: usize = 4096;
/// Error lines kept: enough to hold the error and where it came from.
const KEPT_LINES: usize = 4;
/// A quoted word longer than this is data, not a name.
const MAX_NAME_CHARS: usize = 64;
/// What the user and the parent read of the error.
const MAX_EXCERPT_CHARS: usize = 400;
/// The program is at the start of a command; the rest is not read.
const COMMAND_HEAD_BYTES: usize = 1024;

/// A line with one of these is part of an error.
const ERROR_WORDS: &[&str] = &[
    "error",
    "fail",
    "panic",
    "exception",
    "traceback",
    "fatal",
    "abort",
    "not found",
    "no such",
    "denied",
    "cannot",
    "can't",
    "unable",
    "invalid",
    "refused",
    "undefined",
    "unknown",
    "missing",
    "timed out",
];

/// Shell steps that only prepare the one that runs: `cd dir && cargo test` runs cargo.
const SETUP: &[&str] = &["cd", "pushd", "export", "set", "source", "."];

/// Programs that run the next word: `sudo cargo test` runs cargo. Their options and
/// numbers are skipped; an option's separate value is not known, so `sudo -u me make`
/// reads as `me`, which is only ever alike to itself.
const WRAPPERS: &[&str] = &[
    "sudo", "env", "time", "nice", "nohup", "command", "exec", "timeout",
];

/// The tool that runs commands, whose program is part of the signature.
pub const RUN_COMMAND: &str = "run_command";

pub struct Signature {
    /// Compared between failures.
    pub key: String,
    /// The error lines as the tool wrote them.
    pub excerpt: String,
}

/// The signature of a failed call, or None when its output has no text to recognize,
/// such as a `grep` that found nothing.
pub fn of(tool: &str, arguments: &Value, output: &str) -> Option<Signature> {
    let lines = error_lines(output);
    if lines.is_empty() {
        return None;
    }
    let program = match tool {
        RUN_COMMAND => arguments["command"].as_str().and_then(program),
        _ => None,
    };
    let mut key = format!("{tool}\n{}", program.unwrap_or_default());
    let mut excerpt = String::new();
    for (normalized, raw) in &lines {
        key.push('\n');
        key.push_str(normalized);
        if !excerpt.is_empty() {
            excerpt.push('\n');
        }
        excerpt.push_str(raw);
    }
    Some(Signature {
        key,
        excerpt: truncate(&excerpt, MAX_EXCERPT_CHARS),
    })
}

/// The program a shell command runs: the first word of its first step that is not a
/// setup step (`cd`, `export`, ...), after `NAME=value` assignments and wrappers (`sudo`,
/// `env`, `timeout 60`, ...), without its directory. Steps end at `;`, `&`, `|` and new
/// lines, so `cd x && cargo test 2>&1 | tail` runs `cargo`.
pub fn program(command: &str) -> Option<String> {
    head(command, COMMAND_HEAD_BYTES)
        .split(['\n', ';', '&', '|'])
        .find_map(step_program)
}

fn step_program(step: &str) -> Option<String> {
    let mut wrapped = false;
    for word in step.split_whitespace() {
        let word = word.trim_matches(['"', '\'', '(', ')', '{', '}']);
        let skipped = word.is_empty()
            || is_assignment(word)
            || (wrapped
                && (word.starts_with('-') || word.starts_with(|c: char| c.is_ascii_digit())));
        if skipped {
            continue;
        }
        let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
        if WRAPPERS.contains(&name) {
            wrapped = true;
            continue;
        }
        if SETUP.contains(&name) {
            return None;
        }
        return Some(name.to_owned());
    }
    None
}

fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The last error lines of the output, normalized and as written, oldest first. Lines
/// with an error word win; without any, the last lines stand for the error.
fn error_lines(output: &str) -> Vec<(String, &str)> {
    let lines: Vec<(String, &str)> = tail(output)
        .lines()
        .filter_map(|raw| {
            let normalized = normalize(raw);
            let kept = !normalized.is_empty() && !is_frame(&normalized);
            kept.then(|| (normalized, raw.trim()))
        })
        .collect();
    let errors = lines.iter().filter(|(line, _)| is_error(line)).count();
    let chosen: Vec<(String, &str)> = if errors > 0 {
        lines
            .into_iter()
            .filter(|(line, _)| is_error(line))
            .collect()
    } else {
        lines
    };
    let skip = chosen.len().saturating_sub(KEPT_LINES);
    chosen.into_iter().skip(skip).collect()
}

/// The lines `run_command` puts around the streams carry nothing of the error.
fn is_frame(line: &str) -> bool {
    line.starts_with("exit code:")
        || line == "stdout:"
        || line == "stderr:"
        || line.starts_with("[# bytes cut]")
}

fn is_error(line: &str) -> bool {
    ERROR_WORDS.iter().any(|word| line.contains(word))
}

/// The end of the output from a line start, at most `TAIL_BYTES` long.
fn tail(output: &str) -> &str {
    if output.len() <= TAIL_BYTES {
        return output;
    }
    let mut start = output.len() - TAIL_BYTES;
    while !output.is_char_boundary(start) {
        start += 1;
    }
    let cut = &output[start..];
    // The first line was most likely cut in half.
    cut.split_once('\n').map_or(cut, |(_, rest)| rest)
}

fn head(text: &str, bytes: usize) -> &str {
    if text.len() <= bytes {
        return text;
    }
    let mut end = bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn truncate(text: &str, chars: usize) -> String {
    match text.char_indices().nth(chars) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

/// One line with its incidental details made alike; see the module's doc.
pub fn normalize(line: &str) -> String {
    let unquoted = replace_quotes(&line.to_lowercase());
    let mut out = String::with_capacity(unquoted.len());
    for word in unquoted.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        if word.contains(['/', '\\']) {
            for (index, part) in word.split(':').enumerate() {
                if index > 0 {
                    out.push(':');
                }
                if part.contains(['/', '\\']) {
                    out.push_str("<path>");
                } else {
                    push_numbers(&mut out, part);
                }
            }
        } else {
            push_numbers(&mut out, word);
        }
    }
    out
}

const QUOTES: [(char, char); 5] = [
    ('\'', '\''),
    ('"', '"'),
    ('`', '`'),
    ('\u{2018}', '\u{2019}'),
    ('\u{201c}', '\u{201d}'),
];

/// Quoted text that is not a single name becomes `<q>`. A quote opens only after a
/// non-letter, so the apostrophe of "can't" opens nothing, and only when its closing
/// quote comes later on the line. Linear: each closing quote's last place is known up
/// front, so no search runs past the end in vain.
fn replace_quotes(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut last_close = [None; QUOTES.len()];
    for (index, c) in chars.iter().enumerate() {
        for (slot, (_, close)) in QUOTES.iter().enumerate() {
            if c == close {
                last_close[slot] = Some(index);
            }
        }
    }
    let mut out = String::with_capacity(line.len());
    let mut index = 0;
    while let Some(&c) = chars.get(index) {
        let after_letter = index > 0 && chars[index - 1].is_alphanumeric();
        let opens = QUOTES
            .iter()
            .position(|(open, _)| *open == c)
            .filter(|&slot| !after_letter && last_close[slot].is_some_and(|last| last > index));
        let Some(slot) = opens else {
            out.push(c);
            index += 1;
            continue;
        };
        let close = QUOTES[slot].1;
        let end = (index + 1..chars.len())
            .find(|&at| chars[at] == close)
            .unwrap_or(chars.len());
        let inside = &chars[index + 1..end];
        out.push(c);
        let name = inside.len() <= MAX_NAME_CHARS
            && !inside
                .iter()
                .any(|c| c.is_whitespace() || *c == '/' || *c == '\\');
        if name {
            out.extend(inside);
        } else {
            out.push_str("<q>");
        }
        out.push(close);
        index = end + 1;
    }
    out
}

/// Words are runs of letters, digits and `_`, with a `.` between digits kept in them
/// so a version or a decimal is one word. A uuid is one `#` though it holds `-`.
fn push_numbers(out: &mut String, text: &str) {
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while let Some(&c) = chars.get(index) {
        if !is_word_char(c) {
            out.push(c);
            index += 1;
            continue;
        }
        if is_uuid(&chars[index..]) {
            out.push('#');
            index += UUID_CHARS;
            continue;
        }
        let start = index;
        while let Some(&c) = chars.get(index) {
            let in_number = c == '.'
                && chars[index - 1].is_ascii_digit()
                && chars.get(index + 1).is_some_and(char::is_ascii_digit);
            if !(is_word_char(c) || in_number) {
                break;
            }
            index += 1;
        }
        push_word(out, &chars[start..index]);
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

const UUID_CHARS: usize = 36;

/// `8-4-4-4-12` hex digits that end the word.
fn is_uuid(chars: &[char]) -> bool {
    let Some(uuid) = chars.get(..UUID_CHARS) else {
        return false;
    };
    let shaped = uuid.iter().enumerate().all(|(at, c)| match at {
        8 | 13 | 18 | 23 => *c == '-',
        _ => c.is_ascii_hexdigit(),
    });
    shaped && !chars.get(UUID_CHARS).is_some_and(|c| is_word_char(*c))
}

/// Suffixes that make a number a duration (`0.53s`, `950ms`) or a time in UTC (`02z`).
const UNITS: &[&str] = &[
    "ns", "us", "µs", "ms", "s", "sec", "secs", "m", "min", "h", "z",
];

/// Hex of at least this many characters, with a letter and a digit, is an id: a short
/// commit hash has seven.
const MIN_HEX_ID: usize = 6;

/// A word that is a number becomes `#`: digits, a version (`1.2.3`, `v1.2.3`), an
/// address (`0x7ffd`), a duration, a date-time (`17t10`). A hex id inside a word becomes
/// `#` too (`tmpa81f3c` is `tmp#`). Other digits in a word with a letter or `_` stay, since
/// `test_1` and `test_2` are different things.
fn push_word(out: &mut String, word: &[char]) {
    if !word.iter().any(char::is_ascii_digit) {
        out.extend(word);
        return;
    }
    let number =
        |part: &[char]| !part.is_empty() && part.iter().all(|c| c.is_ascii_digit() || *c == '.');
    let digits = word
        .iter()
        .take_while(|c| c.is_ascii_digit() || **c == '.')
        .count();
    let suffix: String = word[digits..].iter().collect();
    let date_time = word
        .iter()
        .position(|c| *c == 't')
        .is_some_and(|at| number(&word[..at]) && number(&word[at + 1..]));
    let whole = number(word)
        || (word.len() > 2
            && word[..2] == ['0', 'x']
            && word[2..].iter().all(char::is_ascii_hexdigit))
        || (digits > 0 && UNITS.contains(&suffix.as_str()))
        || (word[0] == 'v' && number(&word[1..]))
        || date_time;
    if whole {
        out.push('#');
        return;
    }
    let mut index = 0;
    while index < word.len() {
        let hex = word[index..]
            .iter()
            .take_while(|c| c.is_ascii_hexdigit())
            .count();
        if hex == 0 {
            out.push(word[index]);
            index += 1;
            continue;
        }
        let run = &word[index..index + hex];
        let id = hex >= MIN_HEX_ID
            && run.iter().any(char::is_ascii_digit)
            && run.iter().any(char::is_ascii_alphabetic);
        if id {
            out.push('#');
        } else {
            out.extend(run);
        }
        index += hex;
    }
}
