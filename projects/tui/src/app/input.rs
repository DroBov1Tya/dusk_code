//! The message being typed: text with a cursor, several lines allowed. A long paste
//! stands in the text as one chip character until the message is sent.

use super::paste::{self, PASTE_THRESHOLD, Part};

#[derive(Default)]
pub struct Input {
    pub text: String,
    /// Byte offset into `text`, always on a character boundary.
    pub cursor: usize,
    /// What each chip in `text` stands for, by its index.
    pub pastes: Vec<String>,
}

impl Input {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// The message to send, each chip replaced by its paste in tags.
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        let text = std::mem::take(&mut self.text);
        let pastes = std::mem::take(&mut self.pastes);
        text.chars()
            .map(
                |c| match paste::chip_index(c).and_then(|index| pastes.get(index)) {
                    Some(pasted) => paste::wrap(pasted),
                    None => c.to_string(),
                },
            )
            .collect()
    }

    /// A sent message back in the box, its pasted blocks as chips again.
    pub fn set(&mut self, message: String) {
        self.text.clear();
        self.pastes.clear();
        for part in paste::split(&message) {
            match part {
                Part::Text(text) => self.text.push_str(text),
                Part::Pasted(text) => self.push_chip(text.to_owned()),
            }
        }
        self.cursor = self.text.len();
    }

    /// Long text becomes a chip; short text is typed in.
    pub fn paste(&mut self, text: &str) {
        if text.chars().count() <= PASTE_THRESHOLD {
            return self.insert(text);
        }
        let end = self.text.split_off(self.cursor);
        self.push_chip(text.to_owned());
        self.cursor = self.text.len();
        self.text.push_str(&end);
    }

    /// Replaces a chip with its text, to edit it; the cursor goes after it.
    pub fn expand(&mut self, index: usize) {
        let (Some(chip), Some(pasted)) = (paste::chip(index), self.pastes.get(index)) else {
            return;
        };
        let Some(at) = self.text.find(chip) else {
            return;
        };
        let pasted = pasted.clone();
        self.text.replace_range(at..at + chip.len_utf8(), &pasted);
        self.cursor = at + pasted.len();
    }

    /// The chip right before the cursor, or right after it.
    pub fn chip_at_cursor(&self) -> Option<usize> {
        let before = self.text[..self.cursor].chars().next_back();
        let after = self.text[self.cursor..].chars().next();
        before
            .and_then(paste::chip_index)
            .or_else(|| after.and_then(paste::chip_index))
    }

    /// Too many pastes for the chip characters stay plain text.
    fn push_chip(&mut self, text: String) {
        match paste::chip(self.pastes.len()) {
            Some(chip) => {
                self.text.push(chip);
                self.pastes.push(text);
            }
            None => self.text.push_str(&text),
        }
    }

    pub fn insert(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    pub fn backspace(&mut self) {
        if let Some(previous) = self.previous_boundary() {
            self.text.drain(previous..self.cursor);
            self.cursor = previous;
        }
    }

    pub fn delete(&mut self) {
        if let Some(next) = self.next_boundary() {
            self.text.drain(self.cursor..next);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.previous_boundary().unwrap_or(self.cursor);
    }

    pub fn right(&mut self) {
        self.cursor = self.next_boundary().unwrap_or(self.cursor);
    }

    pub fn home(&mut self) {
        self.cursor = self.line_start();
    }

    pub fn end(&mut self) {
        self.cursor = self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |offset| self.cursor + offset);
    }

    /// Moves to the line above or below, keeping the column where it can. Returns false
    /// when there is no such line.
    pub fn vertical(&mut self, down: bool) -> bool {
        let start = self.line_start();
        let column = self.text[start..self.cursor].chars().count();
        let target = if down {
            match self.text[self.cursor..].find('\n') {
                Some(offset) => self.cursor + offset + 1,
                None => return false,
            }
        } else {
            match start.checked_sub(1) {
                Some(before) => self.text[..before].rfind('\n').map_or(0, |at| at + 1),
                None => return false,
            }
        };
        let line_end = self.text[target..]
            .find('\n')
            .map_or(self.text.len(), |offset| target + offset);
        self.cursor = self.text[target..line_end]
            .char_indices()
            .nth(column)
            .map_or(line_end, |(offset, _)| target + offset);
        true
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind('\n').map_or(0, |at| at + 1)
    }

    fn previous_boundary(&self) -> Option<usize> {
        self.text[..self.cursor]
            .char_indices()
            .last()
            .map(|(at, _)| at)
    }

    fn next_boundary(&self) -> Option<usize> {
        let next = self.text[self.cursor..].chars().next()?;
        Some(self.cursor + next.len_utf8())
    }
}
