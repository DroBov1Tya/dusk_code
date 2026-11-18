//! The one function every text of an episode passes through before it is stored. The
//! text comes from tool output and from the model, and a later turn of another chat
//! reads it, so it is:
//!
//! 1. made plain as a stub is (`message::plain`): one line, no invisible characters, no
//!    brackets or braces of its own;
//! 2. cleared of every value of the project's variables: `<secret>`;
//! 3. cleared of what is shaped like a secret: `<redacted>`. In this order, since each
//!    reads what the one before left: a PEM block; the value after a word such as
//!    `token` and the user part of a URL; a known key prefix and a run of `RUN_CHARS` or
//!    more characters of the base64 alphabet that mixes letters and digits;
//! 4. cut to the field's cap, never inside a character or a mark.
//!
//! Cleaning twice gives what cleaning once gave, and no text makes it panic. The work
//! grows with the length of the text, which is bounded: a look ahead is itself bounded,
//! ends in a jump past what it read, or is remembered for the next one.
//!
//! While it works the two marks are single control characters. Step 1 leaves no control
//! character in the text, so a mark cannot be taken for text, be cut in half, or become
//! part of a later match.

use std::fmt;

use crate::message::plain::plain;

const SECRET_TEXT: &str = "<secret>";
const REDACTED_TEXT: &str = "<redacted>";
/// The marks while the text is worked on.
const SECRET: char = '\u{1}';
const REDACTED: char = '\u{2}';

/// More text than any field keeps; the rest is not read.
const MAX_INPUT_BYTES: usize = 16 * 1024;
/// A variable's value shorter than this is a flag (`1`, `no`, `dev`), not a secret, and
/// replacing it would shred ordinary words.
const MIN_SECRET_CHARS: usize = 4;
/// A run this long of the base64 alphabet, with letters and digits, is taken for a key.
const RUN_CHARS: usize = 24;
/// What follows a known prefix is a key from this length on: `sk-learn` is not one.
const MIN_PREFIX_BODY: usize = 8;
/// How much of a name may follow a word such as `token` (`token_value=`).
const MAX_NAME_TAIL: usize = 32;
/// How many separators may stand between such a word and its value.
const MAX_SEPARATORS: usize = 32;

const PEM_BEGIN: &str = "-----BEGIN";
const PEM_END: &str = "-----END";
const PEM_DASHES: &str = "-----";

/// Prefixes of keys: OpenAI and others, GitHub, AWS, Slack. Matched without case.
const PREFIXES: [&str; 5] = ["github_pat_", "ghp_", "sk-", "akia", "xox"];
/// Words whose value is a secret. Matched without case, anywhere in a name.
const WORDS: [&str; 9] = [
    "authorization",
    "password",
    "api_key",
    "api-key",
    "apikey",
    "passwd",
    "bearer",
    "secret",
    "token",
];
/// After `Authorization:` these are followed by the secret itself.
const SCHEMES: [&str; 4] = ["bearer", "basic", "digest", "token"];

/// A text column of an episode, with the cap its table checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Tool,
    Model,
    Problem,
    Failed,
    Worked,
    Lesson,
}

impl Field {
    /// The most characters the column takes: the checks of the `episodes` table.
    pub const fn cap(self) -> usize {
        match self {
            Self::Tool => 64,
            Self::Model => 128,
            Self::Problem => 400,
            Self::Failed => 600,
            Self::Worked => 300,
            Self::Lesson => 200,
        }
    }
}

/// The values of a project's variables, to be taken out of a text. They are never
/// shown: not in `Debug`, not in a log.
#[derive(Default)]
pub struct Secrets(Vec<String>);

impl Secrets {
    /// The values as they would read in a plain text, the longest first so a value that
    /// starts another does not leave the rest of it behind.
    pub fn new(values: impl IntoIterator<Item = String>) -> Self {
        let mut values: Vec<String> = values
            .into_iter()
            .map(|value| plain(&value))
            .filter(|value| value.chars().count() >= MIN_SECRET_CHARS)
            .collect();
        values.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        values.dedup();
        Self(values)
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Secrets({} hidden)", self.0.len())
    }
}

/// `text` as the column `field` may hold it.
pub fn clean(text: &str, secrets: &Secrets, field: Field) -> String {
    shown(&marked(text, secrets), field.cap())
}

/// `text` cleaned and not cut: what a signature is taken of, so that nothing derived
/// from a secret is stored either.
pub fn scrub(text: &str, secrets: &Secrets) -> String {
    shown(&marked(text, secrets), usize::MAX)
}

/// Steps 1 to 3, with the marks still single characters.
fn marked(text: &str, secrets: &Secrets) -> String {
    let mut text = plain(bounded(text))
        .replace(SECRET_TEXT, SECRET.encode_utf8(&mut [0; 4]))
        .replace(REDACTED_TEXT, REDACTED.encode_utf8(&mut [0; 4]));
    for value in &secrets.0 {
        if text.contains(value.as_str()) {
            text = text.replace(value.as_str(), SECRET.encode_utf8(&mut [0; 4]));
        }
    }
    keys(&in_context(&blocks(&text)))
}

/// At most `MAX_INPUT_BYTES` of `text`, all `clean` reads of it. A word the cut went
/// through is dropped whole: half a key is not recognised as one. Text kept for an
/// episode is bounded by this, never cut shorter, until it is cleaned.
pub fn bounded(text: &str) -> &str {
    if text.len() <= MAX_INPUT_BYTES {
        return text;
    }
    let mut end = MAX_INPUT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let head = part(text, 0, end);
    match head.rfind(char::is_whitespace) {
        Some(space) => part(head, 0, space),
        None => "",
    }
}

/// The marks written out, up to `cap` characters, without space at the ends.
fn shown(marked: &str, cap: usize) -> String {
    let mut out = String::with_capacity(marked.len());
    let mut used = 0usize;
    let mut buffer = [0; 4];
    for c in marked.trim().chars() {
        let (piece, width): (&str, usize) = match c {
            SECRET => (SECRET_TEXT, SECRET_TEXT.len()),
            REDACTED => (REDACTED_TEXT, REDACTED_TEXT.len()),
            c => (c.encode_utf8(&mut buffer), 1),
        };
        if width > cap.saturating_sub(used) {
            break;
        }
        out.push_str(piece);
        used += width;
    }
    out.truncate(out.trim_end().len());
    out
}

/// PEM blocks, each as one mark. They go first: a block is many words, and the value
/// of `key:` would otherwise end at the first of them.
fn blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(c) = from(text, at).chars().next() {
        if let Some(len) = pem(from(text, at)) {
            out.push(REDACTED);
            at += len;
        } else {
            out.push(c);
            at += c.len_utf8();
        }
    }
    out
}

/// What is a key wherever it stands: a long mixed run, a known prefix with a body. They
/// go last: a long name that ends in `_token` is a run too, and its value must be gone
/// before the name is.
fn keys(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    // Where the run that was measured last ends: it is not measured again from inside.
    let mut measured = 0;
    while let Some(c) = from(text, at).chars().next() {
        let rest = from(text, at);
        if at >= measured && in_run(c) {
            let len = run_len(rest);
            if is_key(part(rest, 0, len)) {
                out.push(REDACTED);
                at += len;
                continue;
            }
            measured = at + len;
        }
        if let Some(len) = prefixed(text, at) {
            out.push(REDACTED);
            at += len;
            continue;
        }
        out.push(c);
        at += c.len_utf8();
    }
    out
}

/// What is a secret by what stands before it: the value of a word such as `password`,
/// the user part of a URL. Every such word is read, also one inside the value of
/// another (`token password: x`), and what they cover together goes.
fn in_context(text: &str) -> String {
    let mut ends = WordEnds::new(text);
    let mut spans = Vec::new();
    for (at, _) in text.char_indices() {
        values(text, at, &mut ends, &mut spans);
        if let Some(len) = url_user(from(text, at)) {
            let start = at + "://".len();
            spans.push((start, start + len));
        }
    }
    spans.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    let mut covered = None;
    for (start, end) in spans {
        // A span that starts inside the one before it only makes that one longer.
        if covered.is_none_or(|covered| start > covered) {
            out.push_str(part(text, at, start));
            out.push(REDACTED);
        }
        at = at.max(end);
        covered = Some(at);
    }
    out.push_str(from(text, at));
    out
}

/// The length of the PEM block `rest` starts with: up to the dashes that close its end
/// line, or all of `rest` when the block is cut short.
fn pem(rest: &str) -> Option<usize> {
    if !starts_with(rest, PEM_BEGIN) {
        return None;
    }
    let body = PEM_BEGIN.len();
    let Some(end) = find(from(rest, body), PEM_END) else {
        return Some(rest.len());
    };
    let label = body + end + PEM_END.len();
    match find(from(rest, label), PEM_DASHES) {
        Some(dashes) => Some(label + dashes + PEM_DASHES.len()),
        None => Some(rest.len()),
    }
}

/// The base64 alphabets, the URL one too, and the padding. Hex is inside them.
fn in_run(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-')
}

/// How many bytes of `rest` its first run takes; a run is ASCII, so bytes are characters.
fn run_len(rest: &str) -> usize {
    rest.find(|c| !in_run(c)).unwrap_or(rest.len())
}

fn is_key(run: &str) -> bool {
    run.len() >= RUN_CHARS
        && run.bytes().any(|byte| byte.is_ascii_alphabetic())
        && run.bytes().any(|byte| byte.is_ascii_digit())
}

/// The length of the key that starts at `at` with a known prefix: the prefix and the
/// run after it. Only where a word starts, so `task-` and `risk-` are not `sk-`.
fn prefixed(text: &str, at: usize) -> Option<usize> {
    let starts_word = at
        .checked_sub(1)
        .and_then(|before| text.as_bytes().get(before))
        .is_none_or(|byte| !byte.is_ascii_alphanumeric());
    if !starts_word {
        return None;
    }
    let rest = from(text, at);
    let prefix = PREFIXES.iter().find(|prefix| starts_with(rest, prefix))?;
    let body = run_len(from(rest, prefix.len()));
    (body >= MIN_PREFIX_BODY).then_some(prefix.len() + body)
}

/// Adds the value of the word at `at`, when one of `WORDS` starts there: what follows
/// the rest of a name (`token_value`), then `=`, `:`, quotes or space. A name with a
/// tail needs `=` or `:`, so `tokenizer failed` and `secrets are` have no value. After
/// `Authorization:` a scheme is a value, and the word after it is one too.
fn values(text: &str, at: usize, ends: &mut WordEnds, spans: &mut Vec<(usize, usize)>) {
    let rest = from(text, at);
    let Some(word) = WORDS.iter().find(|word| starts_with(rest, word)) else {
        return;
    };
    let mut start = at + word.len();
    let mut chars = from(text, start).chars().peekable();
    let mut tail = 0;
    while let Some(&c) = chars.peek() {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            break;
        }
        if tail == MAX_NAME_TAIL {
            return;
        }
        tail += 1;
        start += c.len_utf8();
        chars.next();
    }
    let mut separators = 0;
    let mut assigned = false;
    // Quotes right before the value open it and are part of it.
    let mut quotes = 0;
    while let Some(&c) = chars.peek() {
        let quote = is_quote(c);
        if !(quote || c == '=' || c == ':' || c.is_whitespace()) || separators == MAX_SEPARATORS {
            break;
        }
        separators += 1;
        assigned |= c == '=' || c == ':';
        quotes = if quote { quotes + 1 } else { 0 };
        start += c.len_utf8();
        chars.next();
    }
    if separators == 0 || (tail > 0 && !assigned) || start >= text.len() {
        return;
    }
    // A quote is one byte.
    start -= quotes;
    let end = value_end(text, start, ends);
    if end == start {
        return;
    }
    spans.push((start, end));
    let value = part(text, start, end);
    if !SCHEMES
        .iter()
        .any(|scheme| value.eq_ignore_ascii_case(scheme))
    {
        return;
    }
    let mut second = end;
    for c in from(text, end).chars().take(MAX_SEPARATORS) {
        if !c.is_whitespace() {
            break;
        }
        second += c.len_utf8();
    }
    let end = value_end(text, second, ends);
    if end > second {
        spans.push((second, end));
    }
}

/// Where the value that starts at `start` ends: at the next whitespace, and for a value
/// in quotes at the first whitespace after the quote that closes it. A quote left open
/// takes the rest of the text: what it held is not known to have ended.
fn value_end(text: &str, start: usize, ends: &mut WordEnds) -> usize {
    let inside = start + 1;
    match from(text, start).chars().next() {
        Some(quote) if is_quote(quote) => match from(text, inside).find(quote) {
            Some(close) => ends.after(inside + close + 1),
            None => text.len(),
        },
        _ => ends.after(start),
    }
}

fn is_quote(c: char) -> bool {
    matches!(c, '\'' | '"' | '`')
}

/// Where the word at a place ends, for places that are asked in rising order: a word
/// is walked once, however many values end with it.
struct WordEnds<'a> {
    text: &'a str,
    /// The word last walked, from where it was asked to its end.
    walked: Option<(usize, usize)>,
}

impl<'a> WordEnds<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, walked: None }
    }

    /// The first whitespace at or after `start`, or the end of the text.
    fn after(&mut self, start: usize) -> usize {
        if let Some((from, end)) = self.walked
            && (from..=end).contains(&start)
        {
            return end;
        }
        let end = from(self.text, start)
            .find(char::is_whitespace)
            .map_or(self.text.len(), |space| start + space);
        self.walked = Some((start, end));
        end
    }
}

/// The length of the user part of the URL whose `://` `rest` starts with: everything up
/// to the last `@` before the host ends. `https://key@host` has no password and still
/// a secret, so the whole part goes. The look ahead ends at the first `/`, and the
/// next `://` has one, so no character is read twice. It also ends at a mark of
/// `<redacted>`: a later step may put one there, and a second cleaning must read what
/// the first read.
fn url_user(rest: &str) -> Option<usize> {
    let after = rest.strip_prefix("://")?;
    let mut user = None;
    for (index, c) in after.char_indices() {
        if c.is_whitespace() || matches!(c, '/' | '?' | '#' | REDACTED) {
            break;
        }
        if c == '@' && index > 0 {
            user = Some(index);
        }
    }
    user
}

/// Whether `text` starts with the ASCII `prefix`, case aside.
fn starts_with(text: &str, prefix: &str) -> bool {
    text.as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

/// Where the ASCII `needle` first is in `text`, case aside.
fn find(text: &str, needle: &str) -> Option<usize> {
    text.as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

/// `text` from the byte `start`; nothing when that is not a character's start, which
/// ends the caller's loop instead of panicking.
fn from(text: &str, start: usize) -> &str {
    text.get(start..).unwrap_or("")
}

fn part(text: &str, start: usize, end: usize) -> &str {
    text.get(start..end).unwrap_or("")
}

#[cfg(test)]
#[path = "clean_tests.rs"]
pub mod tests;
