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
    /// The other end of a selection, with the cursor as this end.
    pub anchor: Option<usize>,
}

impl Input {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// The message to send, each chip replaced by its paste in tags.
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        self.anchor = None;
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
        self.anchor = None;
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
        self.delete_selection();
        if text.chars().count() <= PASTE_THRESHOLD {
            return self.insert(text);
        }
        let end = self.text.split_off(self.cursor);
        self.push_chip(text.to_owned());
        self.cursor = self.text.len();
        self.text.push_str(&end);
    }

    /// The chip right before the cursor, or right after it.
    pub fn chip_at_cursor(&self) -> Option<usize> {
        let before = self.text[..self.cursor].chars().next_back();
        let after = self.text[self.cursor..].chars().next();
        before
            .and_then(paste::chip_index)
            .or_else(|| after.and_then(paste::chip_index))
    }

    /// Puts edited text behind a chip; text left empty takes the chip away.
    pub fn replace_paste(&mut self, index: usize, text: String) {
        let Some(pasted) = self.pastes.get_mut(index) else {
            return;
        };
        if !text.trim().is_empty() {
            *pasted = text;
            return;
        }
        if let Some(at) = paste::chip(index).and_then(|chip| self.text.find(chip)) {
            let chip_len = paste::chip(index).map_or(0, char::len_utf8);
            self.text.replace_range(at..at + chip_len, "");
            if self.cursor > at {
                self.cursor -= chip_len;
            }
        }
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

    /// The selected byte range, start first, when it is not empty.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor.filter(|&anchor| anchor != self.cursor)?;
        Some((anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    /// The selected text, each chip as the text it stands for.
    pub fn selected(&self) -> Option<String> {
        let (from, to) = self.selection()?;
        let text = self.text[from..to]
            .chars()
            .map(
                |c| match paste::chip_index(c).and_then(|index| self.pastes.get(index)) {
                    Some(pasted) => pasted.clone(),
                    None => c.to_string(),
                },
            )
            .collect();
        Some(text)
    }

    /// Takes the selected text out; false when nothing was selected.
    pub fn delete_selection(&mut self) -> bool {
        let selection = self.selection();
        self.anchor = None;
        let Some((from, to)) = selection else {
            return false;
        };
        self.text.drain(from..to);
        self.cursor = from;
        true
    }

    /// Before a move: a held Shift starts or grows a selection, otherwise it ends.
    pub fn mark(&mut self, extend: bool) {
        self.anchor = match (extend, self.anchor) {
            (true, Some(anchor)) => Some(anchor),
            (true, None) => Some(self.cursor),
            (false, _) => None,
        };
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    /// Puts the cursor on a byte offset, clamped to a character boundary.
    pub fn place(&mut self, offset: usize) {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        self.cursor = offset;
    }

    pub fn insert(&mut self, text: &str) {
        self.delete_selection();
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if let Some(previous) = self.previous_boundary() {
            self.text.drain(previous..self.cursor);
            self.cursor = previous;
        }
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
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
