//! Long pasted text. In the box a paste stands as one chip character, so it moves and
//! deletes as a whole; a sent message carries it in <pasted> tags, which tell the model
//! what it is and let the client fold it, in history as well.

use super::compact_number;

/// Pastes longer than this, in characters, become a chip.
pub const PASTE_THRESHOLD: usize = 1500;

const OPEN: &str = "<pasted>\n";
const CLOSE: &str = "\n</pasted>";
/// Chips are characters from Unicode's private use area, one per paste.
const FIRST_CHIP: u32 = 0xE000;
const LAST_CHIP: u32 = 0xF8FF;

pub enum Part<'a> {
    Text(&'a str),
    Pasted(&'a str),
}

pub fn chip(index: usize) -> Option<char> {
    let code = FIRST_CHIP.checked_add(u32::try_from(index).ok()?)?;
    (code <= LAST_CHIP).then(|| char::from_u32(code)).flatten()
}

pub fn chip_index(c: char) -> Option<usize> {
    let code = u32::from(c);
    (FIRST_CHIP..=LAST_CHIP)
        .contains(&code)
        .then(|| (code - FIRST_CHIP) as usize)
}

pub fn wrap(text: &str) -> String {
    format!("{OPEN}{text}{CLOSE}")
}

/// A message as typed text and pasted blocks, in order. An unclosed tag is plain text.
pub fn split(message: &str) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    let mut rest = message;
    while let Some(start) = rest.find(OPEN) {
        let body = &rest[start + OPEN.len()..];
        let Some(end) = body.find(CLOSE) else {
            break;
        };
        if start > 0 {
            parts.push(Part::Text(&rest[..start]));
        }
        parts.push(Part::Pasted(&body[..end]));
        rest = &body[end + CLOSE.len()..];
    }
    if !rest.is_empty() {
        parts.push(Part::Text(rest));
    }
    parts
}

pub fn has_pasted(message: &str) -> bool {
    split(message)
        .iter()
        .any(|part| matches!(part, Part::Pasted(_)))
}

/// The message without tags: what the user pasted, where they pasted it.
pub fn plain(message: &str) -> String {
    split(message)
        .into_iter()
        .map(|part| match part {
            Part::Text(text) | Part::Pasted(text) => text,
        })
        .collect()
}

/// The message with each paste as its label, for previews.
pub fn labelled(message: &str) -> String {
    split(message)
        .into_iter()
        .map(|part| match part {
            Part::Text(text) => text.to_owned(),
            Part::Pasted(text) => format!("[{}]", label(text)),
        })
        .collect()
}

pub fn label(text: &str) -> String {
    let lines = text.lines().count();
    let chars = compact_number(text.chars().count() as u64);
    format!("pasted text \u{00B7} {lines} lines \u{00B7} {chars} chars")
}
