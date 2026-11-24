//! The model's text going round: one piece written again and again, as a model that
//! lost its way does, burning the output cap on a loop no tool rule sees. As Gemini CLI
//! does, the streamed answer is read in 50-character pieces, each piece's places kept,
//! and a piece that comes back `limits.text_loop_repeats` times close together (on
//! average within `MAX_GAP` characters of the one before) is a loop when what lies
//! between its places is mostly the same too: a list of paths or links that share a
//! long start says something new after each. Only places that do not overlap count, so
//! a run of one character or a short pattern is a loop only once it is ten pieces long.
//!
//! Text that repeats by its nature is not read: code (inside a fence or indented),
//! tables, lists, headings, quotes and rules. Such a line also forgets what came
//! before it, so items of a list or rows of a table never add up to a loop with the
//! prose around them. Only the place of a line's start decides what it is, so a line is
//! held back until its first few characters are known.
//!
//! What the model wrote is untrusted and may be long: everything kept is bounded, and
//! nothing here can fail on any text.

use std::collections::{HashMap, VecDeque};

use super::hash;

/// Characters a piece holds.
pub const CHUNK: usize = 50;
/// The most a piece may on average move from one return to the next and still count as
/// close together: a paragraph of up to five pieces said again and again.
const MAX_GAP: usize = 5 * CHUNK;
/// Characters of a line, after its indent, that tell what kind of line it is.
const DECIDE_CHARS: usize = 8;
/// A line's start held back at most: an indent this long is code anyway.
const MAX_HELD: usize = 64;
/// Characters read between two sweeps of places too old to matter.
const SWEEP_EVERY: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line {
    Prose,
    Skipped,
}

pub struct Chant {
    /// Times a piece must come back to make a loop.
    repeats: usize,
    /// The start of the line being written, until its kind is known.
    held: String,
    /// The kind of the line being written, once known.
    line: Option<Line>,
    in_code: bool,
    /// The newest prose characters, at most a piece.
    piece: VecDeque<char>,
    /// Where each piece started, its newest `repeats` places, by the piece's hash.
    places: HashMap<u64, VecDeque<usize>>,
    /// The newest prose characters with their places, as far back as a loop reaches.
    recent: VecDeque<(usize, char)>,
    /// Characters of the answer read so far.
    read: usize,
    /// Prose characters read so far.
    fed: usize,
    /// Where the answer should be cut, once it went round.
    cut: Option<usize>,
}

impl Chant {
    pub fn new(repeats: u32) -> Self {
        Self {
            repeats: usize::try_from(repeats).unwrap_or(usize::MAX).max(2),
            held: String::new(),
            line: None,
            in_code: false,
            piece: VecDeque::with_capacity(CHUNK + 1),
            places: HashMap::new(),
            recent: VecDeque::new(),
            read: 0,
            fed: 0,
            cut: None,
        }
    }

    /// Reads the next part of the answer; true once it went round, the first time.
    pub fn push(&mut self, text: &str) -> bool {
        if self.cut.is_some() {
            return false;
        }
        for c in text.chars() {
            self.char(c);
            if self.cut.is_some() {
                return true;
            }
        }
        false
    }

    /// The characters of the answer to keep once it went round: up to the second time
    /// the piece came, so what it said is there once.
    pub fn cut(&self) -> Option<usize> {
        self.cut
    }

    fn char(&mut self, c: char) {
        let at = self.read;
        self.read += 1;
        if c == '\n' {
            if self.line.is_none() {
                self.decide(true);
            }
            if self.line == Some(Line::Prose) {
                self.prose(c, at);
            }
            self.held.clear();
            self.line = None;
            return;
        }
        match self.line {
            Some(Line::Prose) => self.prose(c, at),
            Some(Line::Skipped) => {}
            None => {
                self.held.push(c);
                self.decide(false);
                if self.line == Some(Line::Prose) {
                    // The held start was not read yet; it ends at `at`.
                    let held = std::mem::take(&mut self.held);
                    let first = at + 1 - held.chars().count();
                    for (i, c) in held.chars().enumerate() {
                        self.prose(c, first + i);
                        if self.cut.is_some() {
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Tells the kind of the line held, when its start says it or it `ended`.
    fn decide(&mut self, ended: bool) {
        let held = self.held.as_str();
        let rest = held.trim_start();
        let indent = held.len() - rest.len();
        let fence = rest.starts_with("```") || rest.starts_with("~~~");
        let known = ended || rest.chars().count() >= DECIDE_CHARS || held.len() >= MAX_HELD;
        let kind = if fence {
            self.in_code = !self.in_code;
            Line::Skipped
        } else if self.in_code || held.starts_with('\t') || indent >= 4 {
            Line::Skipped
        } else if rest.is_empty() {
            if !known {
                return;
            }
            Line::Prose
        } else if structured(rest) {
            Line::Skipped
        } else if rest.chars().all(rule_char) {
            // A rule, or a line of marks still to tell.
            if !known {
                return;
            }
            Line::Skipped
        } else if !known && could_be_list(rest) {
            return;
        } else {
            Line::Prose
        };
        if kind == Line::Skipped && !self.in_code || fence {
            self.forget();
        }
        self.line = Some(kind);
    }

    fn forget(&mut self) {
        self.piece.clear();
        self.places.clear();
        self.recent.clear();
    }

    fn prose(&mut self, c: char, at: usize) {
        self.piece.push_back(c);
        if self.piece.len() > CHUNK {
            self.piece.pop_front();
        }
        if self.recent.len() > self.reach().saturating_add(CHUNK) {
            self.recent.pop_front();
        }
        self.recent.push_back((at, c));
        self.fed += 1;
        if self.fed.is_multiple_of(SWEEP_EVERY) {
            self.sweep(at);
        }
        if self.piece.len() < CHUNK {
            return;
        }
        let start = at + 1 - CHUNK;
        let key = hash(&self.piece);
        let places = self.places.entry(key).or_default();
        // Only places that do not overlap count: inside a run of one character, or of a
        // short pattern, every piece is the same one place later, and a long hash, a
        // rule after a word or a bar of blocks is no loop.
        if places.back().is_some_and(|last| start < last + CHUNK) {
            return;
        }
        places.push_back(start);
        if places.len() > self.repeats {
            places.pop_front();
        }
        if places.len() < self.repeats {
            return;
        }
        let (Some(&first), Some(&last), Some(&second)) =
            (places.front(), places.back(), places.get(1))
        else {
            return;
        };
        let close = last.saturating_sub(first) <= MAX_GAP * (self.repeats - 1);
        if close && self.places.get(&key).is_some_and(|places| self.spans_alike(places)) {
            self.cut = Some(second);
        }
    }

    /// Whether most of the text between `places` of a piece that came back is the same:
    /// a loop says one thing again, a list says a new thing after a shared start.
    fn spans_alike(&self, places: &VecDeque<usize>) -> bool {
        let spans: Vec<String> = places
            .iter()
            .zip(places.iter().skip(1))
            .map(|(from, to)| self.text(*from, *to))
            .collect();
        let Some(newest) = spans.last() else {
            return false;
        };
        let alike = spans.iter().filter(|span| *span == newest).count();
        alike * 2 > spans.len()
    }

    /// The prose characters kept from place `from` up to `to`.
    fn text(&self, from: usize, to: usize) -> String {
        let start = self.recent.partition_point(|(at, _)| *at < from);
        let end = self.recent.partition_point(|(at, _)| *at < to);
        self.recent.range(start..end).map(|(_, c)| *c).collect()
    }

    /// Characters a loop may span: its places on average `MAX_GAP` apart.
    fn reach(&self) -> usize {
        MAX_GAP.saturating_mul(self.repeats)
    }

    /// Forgets pieces too far back to make a loop with anything still to come.
    fn sweep(&mut self, at: usize) {
        let reach = self.reach();
        self.places
            .retain(|_, places| places.back().is_some_and(|last| last + reach >= at));
    }
}

/// A table row, a heading, a quote or a list item, by its start after the indent.
fn structured(rest: &str) -> bool {
    if rest.starts_with(['|', '#', '>']) {
        return true;
    }
    let mut chars = rest.chars();
    match chars.next() {
        Some('-' | '*' | '+' | '•') => chars.next() == Some(' '),
        Some(c) if c.is_ascii_digit() => {
            let after = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            after.starts_with(". ") || after.starts_with(") ")
        }
        _ => false,
    }
}

/// Whether more of the line could still make it a numbered item: digits so far, then
/// maybe the mark.
fn could_be_list(rest: &str) -> bool {
    let after = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    after.len() < rest.len() && matches!(after, "" | "." | ")")
}

fn rule_char(c: char) -> bool {
    matches!(c, '-' | '_' | '=' | '*' | '+' | '~') || ('\u{2500}'..='\u{257f}').contains(&c)
}

/// What the model reads when its answer was cut for going round.
pub fn note(repeats: u32) -> String {
    format!(
        "[your last answer was cut: one piece of it came back {repeats} times. Do not write \
         it again. Say what is new in a few lines, or take the next action.]"
    )
}
