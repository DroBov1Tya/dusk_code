//! Long pasted text in the box. A paste stands as one chip character, so it moves and
//! deletes as a whole; the core's <pasted> tags carry it in the sent message, and how a
//! paste looks on screen is worked out here.

pub use dusk_core::chat::pasted::{Part, has_pasted, plain, split, wrap};

use super::compact_number;

/// Pastes longer than this, in characters, become a chip.
pub const PASTE_THRESHOLD: usize = 1500;

/// Chips are characters from Unicode's private use area, one per paste.
const FIRST_CHIP: u32 = 0xE000;
const LAST_CHIP: u32 = 0xF8FF;

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
