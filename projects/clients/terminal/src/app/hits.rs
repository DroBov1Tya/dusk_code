//! Focus, activity and the clickable places of the last drawn frame.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Input,
    /// Walking the foldable entries; the index is the selected one.
    Browse(usize),
}

#[derive(Clone, PartialEq, Eq)]
pub enum Activity {
    Idle,
    Thinking,
    Writing,
    Tool(String),
    Compacting,
    /// /fork is describing the thread for a new one.
    Forking,
    Retrying(u32),
}

/// The button pressed last, which darkens for `PRESS` after the click.
pub struct Pressed {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub at: Instant,
}

/// How long a pressed button stays dark: long enough to see, short enough not to wait.
pub const PRESS: Duration = Duration::from_millis(140);

/// A button on the entry under the mouse, on the last drawn frame.
pub struct ButtonHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub index: usize,
    pub button: Button,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// Folds any entry into one line, wherever it is scrolled to.
    Hide,
    Copy,
    /// Puts a sent message back into the box.
    Edit,
}

/// A paste chip in the message box on the last drawn frame.
pub struct ChipHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub index: usize,
}

/// A row of the message box on the last drawn frame: where each character starts on
/// screen with its byte offset, and the offset a click past the right end goes to.
pub struct InputRow {
    pub row: u16,
    pub right: u16,
    pub cells: Vec<(u16, usize)>,
    pub end: usize,
}

/// Text marked with the mouse, as (transcript line, screen column) at both ends.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: (usize, u16),
    pub cursor: (usize, u16),
}

impl Selection {
    /// Both ends, top first.
    pub fn ordered(self) -> ((usize, u16), (usize, u16)) {
        if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }
}

/// Transcript columns left of this hold bars and indents, not text, so a selection
/// starts after them.
pub const TEXT_COLUMN: u16 = 3;

/// The header row of a foldable entry on the last drawn frame.
pub struct TranscriptHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub index: usize,
}
