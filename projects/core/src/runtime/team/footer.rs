//! The labelled lines that end a member's answer (`Status:`, `Verdict:`, `Checked:` and
//! the rest) and the finding lines right above them, read the same way for a hand-in, a
//! review and the critic. Models write them with markdown: a bullet, a bold label, a
//! blank line between, a value on the lines under its label. All of that is read; what
//! is not is a label further up, such as in a file the answer quotes, and a word that is
//! not one of the words asked for.
//!
//! The footer is found from the end: label lines, each with the lines under it, up to
//! the first line that is neither. Lines under a label belong to it when another label
//! follows them; under the last label only a list, or any lines when the label has no
//! value of its own, plus one line of sign-off. A label seen twice ends the footer: the
//! one further up is the answer's body, unless the two stand together and disagree,
//! which is no answer at all (`ambiguous`).

/// Lines under one label at most, and lines of a footer as a whole.
const MAX_UNDER: usize = 12;
const MAX_LINES: usize = 64;

/// One label and what it says: its own value first, then each line under it, made
/// plain of list markers. Empty parts are left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Field {
    pub label: &'static str,
    pub parts: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Footer<'a> {
    /// In the answer's order.
    pub fields: Vec<Field>,
    /// The finding lines as label and value, in the answer's order.
    pub items: Vec<(&'static str, String)>,
    /// The finding lines and the footer as written, trimmed and without blank lines.
    pub lines: Vec<&'a str>,
    /// Where `lines` starts among the answer's lines, trimmed and without blank ones.
    pub top: usize,
    /// One label stands twice in a row with two different words.
    pub ambiguous: bool,
}

impl Footer<'_> {
    /// Everything `label` says, its parts joined by `; `.
    pub fn text(&self, label: &str) -> Option<String> {
        let field = self.fields.iter().find(|field| field.label == label)?;
        Some(field.parts.join("; "))
    }

    /// The one word `label` says, lowercase, when it says exactly one of `words`; None
    /// when it is missing, says something else, or the footer is ambiguous.
    pub fn word(&self, label: &str, words: &[&str]) -> Option<String> {
        if self.ambiguous {
            return None;
        }
        let field = self.fields.iter().find(|field| field.label == label)?;
        let said = word(field.parts.first()?);
        words.contains(&said.as_str()).then_some(said)
    }
}

impl Footer<'_> {
    /// The remark `label` makes after its one word (`approve - but fix the docs`), empty
    /// when it makes none.
    pub fn remark(&self, label: &str) -> Option<&str> {
        let field = self.fields.iter().find(|field| field.label == label)?;
        Some(said(field.parts.first()?).1)
    }
}

/// Reads the footer of `answer`, whose labels are `labels` (with their colon), and the
/// finding lines labelled `items` right above it or among it.
pub(super) fn read<'a>(
    answer: &'a str,
    labels: &[&'static str],
    items: &[&'static str],
) -> Footer<'a> {
    let lines: Vec<&str> = answer
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let mut footer = Footer::default();
    // Lines not yet given to a label, nearest first.
    let mut under: Vec<&str> = Vec::new();
    // Where the footer starts so far, and where a sign-off line ends it.
    let mut top = lines.len();
    let mut end = lines.len();
    let mut at = lines.len();
    while at > 0 && lines.len() - at < MAX_LINES {
        let line = lines[at - 1];
        if let Some((label, value)) = labelled(line, labels) {
            if let Some(seen) = footer.fields.iter().find(|field| field.label == label) {
                let together = under.is_empty();
                let first = seen.parts.first().map(String::as_str).unwrap_or_default();
                footer.ambiguous |= together && word(first) != word(value);
                break;
            }
            let last = footer.fields.is_empty() && footer.items.is_empty();
            if !under.is_empty() && !(belongs(&under, last, value) || last && under.len() == 1) {
                break;
            }
            if last && under.len() == 1 && !belongs(&under, last, value) {
                // A sign-off after the footer: not part of any label.
                end = at;
                under.clear();
            }
            let mut parts: Vec<String> = Vec::new();
            if !value.is_empty() {
                parts.push(value.to_owned());
            }
            parts.extend(under.drain(..).rev().map(|line| unlisted(line).to_owned()));
            footer.fields.insert(0, Field { label, parts });
            top = at - 1;
        } else if let Some((label, value)) = item_of(line, items) {
            if value.is_empty() {
                // A heading of findings (`Missing:`, `## Missing`): each line under it
                // is one finding.
                let found: Vec<_> = under
                    .drain(..)
                    .rev()
                    .map(|line| (label, unlisted(line).to_owned()))
                    .collect();
                footer.items.splice(0..0, found);
            } else if !under.is_empty() {
                // A finding line stands alone: lines under it are no part of it.
                break;
            } else {
                footer.items.insert(0, (label, value.to_owned()));
            }
            top = at - 1;
        } else {
            under.push(line);
            // A long list may still be the findings of a heading further up.
            if under.len() > MAX_UNDER && !under.iter().all(|line| listed(line)) {
                break;
            }
        }
        at -= 1;
    }
    if footer.fields.is_empty() {
        footer.items.clear();
        return footer;
    }
    footer.lines = lines[top..end].to_vec();
    footer.top = top;
    footer
}

/// Whether the lines under a label are its own: anything between two labels, and under
/// the last label a list, or anything when the label has no value of its own.
fn belongs(under: &[&str], last: bool, value: &str) -> bool {
    under.len() <= MAX_UNDER && (!last || value.is_empty() || under.iter().all(|line| listed(line)))
}

/// The label of `line` among `labels` and its value: `Status: done`, `- Status: done`,
/// `**Status:** done`, `**Status**: done` and `status: done` alike. A quote (`> `) is
/// someone else's words and is never read.
fn labelled<'a>(line: &'a str, labels: &[&'static str]) -> Option<(&'static str, &'a str)> {
    let line = unlisted(line).trim_start_matches(['*', '_', '#', ' ']);
    labels.iter().find_map(|&label| {
        let name = label.trim_end_matches(':');
        let head = line.get(..name.len())?;
        if !head.eq_ignore_ascii_case(name) {
            return None;
        }
        let rest = line[name.len()..]
            .trim_start_matches(['*', '_'])
            .trim_start();
        let rest = rest.strip_prefix(':')?;
        Some((label, rest.trim_start_matches(['*', '_']).trim()))
    })
}

/// A finding line among `items` and its value, or a heading of findings with no value:
/// `Missing:` on its own, `## Missing`, `**Missing**`.
pub(super) fn item_of<'a>(
    line: &'a str,
    items: &[&'static str],
) -> Option<(&'static str, &'a str)> {
    let line = line.trim();
    if let Some(found) = labelled(line, items) {
        return Some(found);
    }
    let bold =
        line.starts_with(['*', '_']) && line.trim_end_matches([':', ' ']).ends_with(['*', '_']);
    if !line.starts_with('#') && !bold {
        return None;
    }
    let name = line
        .trim_start_matches(['#', '*', '_', ' '])
        .trim_end_matches(['*', '_', ':', ' ']);
    let first = name.split_whitespace().next()?;
    items
        .iter()
        .find(|label| first.eq_ignore_ascii_case(label.trim_end_matches(':')))
        .map(|&label| (label, ""))
}

/// A list item: `- x`, `* x`, `+ x`, `• x` or `1. x`.
pub(super) fn listed(line: &str) -> bool {
    unlisted(line).len() != line.len()
}

/// The line without its list marker.
pub(super) fn unlisted(line: &str) -> &str {
    for marker in ["- ", "* ", "+ ", "\u{2022} "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return rest.trim_start();
        }
    }
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    match line[digits..].strip_prefix(". ") {
        Some(rest) if digits > 0 => rest.trim_start(),
        _ => line,
    }
}

/// The label of `line` among `labels` and its value, for a line read alone, such as a
/// stored footer's: markdown is read as `read` reads it.
pub(super) fn label_of<'a>(
    line: &'a str,
    labels: &[&'static str],
) -> Option<(&'static str, &'a str)> {
    labelled(line.trim(), labels)
}

/// A value as one word: without markdown around it or a full stop after it, lowercase.
/// Words after it set apart by a bracket, a dash, a colon or a full stop are a remark
/// (`done (all tests pass)`, `done - tests pass`); an emoji or a `!` after it is no part
/// of it; any other words make it no one word (`done or blocked`).
fn word(value: &str) -> String {
    said(value).0
}

/// A value as its one word, as `word` reads it, and the remark after that word, empty
/// when it has none.
pub(super) fn said(value: &str) -> (String, &str) {
    let value = value.trim().trim_matches(['*', '_', '`']).trim();
    let cut = value
        .find(|c: char| c.is_whitespace() || matches!(c, '(' | ':'))
        .unwrap_or(value.len());
    let (head, rest) = value.split_at(cut);
    let rest = rest.trim();
    let remark = rest.starts_with(['(', '-', '\u{2013}', '\u{2014}', ':'])
        || head.trim_end_matches(['*', '_', '`']).ends_with('.');
    let adorned = rest.chars().all(adornment);
    match !head.is_empty() && (remark || adorned) {
        true => (bare(head), if adorned { "" } else { rest }),
        false => (bare(value), ""),
    }
}

/// An emoji, a mark, a `!` or a `.` after a word: no part of what it says.
fn adornment(c: char) -> bool {
    c.is_whitespace()
        || matches!(c, '!' | '.' | '*' | '_' | '`')
        || !c.is_ascii() && !c.is_alphanumeric()
}

/// A word without the markdown and adornment around it, lowercase.
fn bare(word: &str) -> String {
    word.trim()
        .trim_matches(['*', '_', '`'])
        .trim_end_matches(adornment)
        .trim_matches(['*', '_', '`'])
        .trim()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABELS: [&str; 3] = ["Status:", "Checked:", "Not checked:"];

    #[test]
    fn markdown_labels_and_values_are_read() {
        for answer in [
            "Status: done\nChecked: ran it",
            "- Status: done\n- Checked: ran it",
            "**Status:** done\n**Checked:** ran it",
            "**Status**: **Done**.\n\n\nChecked: ran it",
            "STATUS: done\n* checked: ran it",
            "### Status: done\nChecked: ran it",
        ] {
            let footer = read(answer, &LABELS, &[]);
            assert_eq!(
                footer.word("Status:", &["done"]).as_deref(),
                Some("done"),
                "{answer}"
            );
            assert_eq!(
                footer.text("Checked:").as_deref(),
                Some("ran it"),
                "{answer}"
            );
        }
    }

    #[test]
    fn a_remark_after_the_word_is_left_out() {
        for answer in [
            "Status: done (all tests pass)",
            "Status: done - tests pass",
            "Status: **done** \u{2014} tests pass",
            "Status: done: tests pass",
            "Status: Done. All tests pass.",
            "Status: done \u{2705}",
            "Status: **Done!**",
        ] {
            let footer = read(answer, &LABELS, &[]);
            assert_eq!(
                footer.word("Status:", &["done", "blocked"]).as_deref(),
                Some("done"),
                "{answer}"
            );
        }
        // A second word that is no remark is no answer, nor is a word with more to it.
        for answer in [
            "Status: done or blocked",
            "Status: done-ish",
            "Status: done, blocked",
        ] {
            let footer = read(answer, &LABELS, &[]);
            assert_eq!(
                footer.word("Status:", &["done", "blocked"]),
                None,
                "{answer}"
            );
        }
    }

    #[test]
    fn a_value_may_take_the_lines_under_its_label() {
        let answer = "Done.\nStatus:\ndone\nChecked:\n- cargo test: 12 passed\n- clippy: clean\n\
                      Not checked: nothing";
        let footer = read(answer, &LABELS, &[]);
        assert_eq!(footer.word("Status:", &["done"]).as_deref(), Some("done"));
        assert_eq!(
            footer.text("Checked:").as_deref(),
            Some("cargo test: 12 passed; clippy: clean")
        );
        assert_eq!(footer.lines.first(), Some(&"Status:"));
        // Under the last label: a list, or anything when it says nothing itself.
        let listed = read(
            "Status: done\nChecked: ran\n1. the tests\n2. clippy",
            &LABELS,
            &[],
        );
        assert_eq!(
            listed.text("Checked:").as_deref(),
            Some("ran; the tests; clippy")
        );
        let bare = read("Status: done\nChecked:\nthe tests\nclippy", &LABELS, &[]);
        assert_eq!(bare.text("Checked:").as_deref(), Some("the tests; clippy"));
    }

    #[test]
    fn a_sign_off_is_no_part_of_the_footer_but_prose_ends_it() {
        let signed = read("Status: done\nChecked: ran it\nThanks!", &LABELS, &[]);
        assert_eq!(signed.text("Checked:").as_deref(), Some("ran it"));
        assert_eq!(signed.lines, ["Status: done", "Checked: ran it"]);
        // Lines of prose after the last label: the labels were the body's.
        let prose = read("Status: blocked\nChecked: x\none\ntwo", &LABELS, &[]);
        assert_eq!(prose, Footer::default());
    }

    #[test]
    fn what_is_not_one_clear_word_is_no_answer() {
        for answer in [
            "Status: done or blocked",
            "Status: blocked?",
            "Status: done\nStatus: blocked",
            "> Status: done",
            "Statuses: done",
            "Status code: done",
        ] {
            let footer = read(answer, &LABELS, &[]);
            assert_eq!(
                footer.word("Status:", &["done", "blocked"]),
                None,
                "{answer}"
            );
        }
        // The same word twice is still that word; a label further up is the body's.
        let twice = read("Status: done\nStatus: Done.", &LABELS, &[]);
        assert_eq!(twice.word("Status:", &["done"]).as_deref(), Some("done"));
        let quoted = read("Status: blocked\nthe log ends\nStatus: done", &LABELS, &[]);
        assert_eq!(
            quoted.word("Status:", &["done", "blocked"]).as_deref(),
            Some("done")
        );
    }

    #[test]
    fn finding_lines_stand_above_or_among_the_labels() {
        let items = ["Problem:"];
        let answer = "Problem: high | quoted\nprose\n- **Problem:** high | a | b | c\n\
                      Status: done\nProblem: low | d | e | f\nChecked: x";
        let footer = read(answer, &LABELS, &items);
        let found: Vec<&str> = footer
            .items
            .iter()
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(found, ["high | a | b | c", "low | d | e | f"]);
        // A heading of findings takes the lines under it, one finding each.
        let gaps = ["Missing:", "Unverified:"];
        for answer in [
            "Missing:\n- a\n- b\nUnverified:\n- c\nStatus: done",
            "## Missing\n- a\n- b\n## Unverified:\n- c\nStatus: done",
            "**Missing**\na\nb\n**Unverified:**\n* c\nStatus: done",
        ] {
            let footer = read(answer, &LABELS, &gaps);
            assert_eq!(
                footer.items,
                [
                    ("Missing:", "a".to_owned()),
                    ("Missing:", "b".to_owned()),
                    ("Unverified:", "c".to_owned())
                ],
                "{answer}"
            );
            assert_eq!(footer.top, 0, "{answer}");
        }
        // Without a label there is no footer, and no finding either.
        assert_eq!(
            read("Problem: high | a", &LABELS, &items),
            Footer::default()
        );
    }
}
