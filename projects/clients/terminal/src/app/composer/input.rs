//! The message being typed: text with a cursor, several lines allowed. A long paste
//! stands in the text as one chip character until the message is sent.

use super::paste::{self, PASTE_THRESHOLD, Part};

/// Undo steps kept; older ones are dropped.
const UNDO_DEPTH: usize = 100;

/// What Ctrl+Z puts back.
struct Snapshot {
    text: String,
    pastes: Vec<String>,
    cursor: usize,
}

/// Typing or deleting on from where the last such edit ended joins it as one undo step.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Typing,
    Deleting,
    Other,
}

#[derive(Default)]
pub struct Input {
    pub text: String,
    /// The slash prefix and candidate selected by repeated Tab presses.
    pub command_cycle: Option<(String, usize)>,
    /// Byte offset into `text`, always on a character boundary.
    pub cursor: usize,
    /// What each chip in `text` stands for, by its index.
    pub pastes: Vec<String>,
    /// The other end of a selection, with the cursor as this end.
    pub anchor: Option<usize>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The last edit and where the cursor stood after it.
    last_edit: Option<(Kind, usize)>,
}

impl Input {
    /// Text to edit, the cursor at its start and nothing to undo yet.
    pub fn holding(text: String) -> Self {
        Self {
            text,
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    /// The message to send, each chip replaced by its paste in tags.
    /// Undo brings the message back, so one sent or cleared by mistake is not lost.
    pub fn take(&mut self) -> String {
        self.command_cycle = None;
        self.anchor = None;
        let mut message = String::new();
        self.edit(Kind::Other, |input| {
            input.cursor = 0;
            let text = std::mem::take(&mut input.text);
            let pastes = std::mem::take(&mut input.pastes);
            message = text
                .chars()
                .map(
                    |c| match paste::chip_index(c).and_then(|index| pastes.get(index)) {
                        Some(pasted) => paste::wrap(pasted),
                        None => c.to_string(),
                    },
                )
                .collect();
        });
        message
    }

    /// A sent message back in the box, its pasted blocks as chips again.
    pub fn set(&mut self, message: String) {
        self.command_cycle = None;
        self.anchor = None;
        self.edit(Kind::Other, |input| {
            input.text.clear();
            input.pastes.clear();
            for part in paste::split(&message) {
                match part {
                    Part::Text(text) => input.text.push_str(text),
                    Part::Pasted(text) => input.push_chip(text.to_owned()),
                    Part::Source(name) => input.text.push_str(&paste::source_tag(name)),
                    // Sent again, the path is kept again.
                    Part::Sound { path, .. } => input.text.push_str(path),
                }
            }
            input.cursor = input.text.len();
        });
    }

    /// Long text becomes a chip; short text is typed in.
    pub fn paste(&mut self, text: &str) {
        self.command_cycle = None;
        self.edit(Kind::Other, |input| {
            input.cut_selection();
            if text.chars().count() <= PASTE_THRESHOLD {
                input.text.insert_str(input.cursor, text);
                input.cursor += text.len();
                return;
            }
            let end = input.text.split_off(input.cursor);
            input.push_chip(text.to_owned());
            input.cursor = input.text.len();
            input.text.push_str(&end);
        });
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
            self.edit(Kind::Other, |input| {
                input.text.replace_range(at..at + chip_len, "");
                if input.cursor > at {
                    input.cursor -= chip_len;
                }
            });
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
        let mut cut = false;
        self.edit(Kind::Other, |input| cut = input.cut_selection());
        cut
    }

    fn cut_selection(&mut self) -> bool {
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
        self.command_cycle = None;
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        self.cursor = offset;
    }

    pub fn insert(&mut self, text: &str) {
        self.command_cycle = None;
        // A word typed on is one step; a space or a new line starts the next.
        let mut chars = text.chars();
        let kind = match (chars.next(), chars.next(), self.selection()) {
            (Some(c), None, None) if !c.is_whitespace() => Kind::Typing,
            _ => Kind::Other,
        };
        self.edit(kind, |input| {
            input.cut_selection();
            input.text.insert_str(input.cursor, text);
            input.cursor += text.len();
        });
    }

    pub fn backspace(&mut self) {
        self.command_cycle = None;
        if self.delete_selection() {
            return;
        }
        if let Some(previous) = self.previous_boundary() {
            self.edit(Kind::Deleting, |input| {
                input.text.drain(previous..input.cursor);
                input.cursor = previous;
            });
        }
    }

    pub fn delete(&mut self) {
        self.command_cycle = None;
        if self.delete_selection() {
            return;
        }
        if let Some(next) = self.next_boundary() {
            self.edit(Kind::Deleting, |input| {
                input.text.drain(input.cursor..next);
            });
        }
    }

    /// Runs a change of the text and keeps what was there for Ctrl+Z.
    fn edit(&mut self, kind: Kind, change: impl FnOnce(&mut Self)) {
        let before = self.snapshot();
        change(self);
        if self.text == before.text && self.pastes == before.pastes {
            return;
        }
        let joins = kind != Kind::Other && self.last_edit == Some((kind, before.cursor));
        if !joins {
            if self.undo.len() == UNDO_DEPTH {
                self.undo.remove(0);
            }
            self.undo.push(before);
        }
        self.redo.clear();
        self.last_edit = Some((kind, self.cursor));
    }

    /// Goes one step back; false when there is none.
    pub fn undo(&mut self) -> bool {
        let Some(earlier) = self.undo.pop() else {
            return false;
        };
        let now = self.snapshot();
        self.redo.push(now);
        self.restore(earlier);
        true
    }

    /// Takes back an undo; false when there is none.
    pub fn redo(&mut self) -> bool {
        let Some(later) = self.redo.pop() else {
            return false;
        };
        let now = self.snapshot();
        self.undo.push(now);
        self.restore(later);
        true
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            pastes: self.pastes.clone(),
            cursor: self.cursor,
        }
    }

    fn restore(&mut self, snapshot: Snapshot) {
        self.text = snapshot.text;
        self.pastes = snapshot.pastes;
        self.cursor = snapshot.cursor;
        self.anchor = None;
        self.command_cycle = None;
        self.last_edit = None;
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

    /// Moves to the line above or below, keeping the column where it can. With no such
    /// line it goes to the end or the start of the text first, as editors do. Returns
    /// false when it is there already.
    pub fn vertical(&mut self, down: bool) -> bool {
        let start = self.line_start();
        let column = self.text[start..self.cursor].chars().count();
        let edge = if down { self.text.len() } else { 0 };
        let target = if down {
            self.text[self.cursor..]
                .find('\n')
                .map(|offset| self.cursor + offset + 1)
        } else {
            start
                .checked_sub(1)
                .map(|before| self.text[..before].rfind('\n').map_or(0, |at| at + 1))
        };
        let Some(target) = target else {
            let moved = self.cursor != edge;
            self.cursor = edge;
            return moved;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn past_the_last_line_the_cursor_goes_to_the_end_first() {
        let mut input = Input {
            text: "one\ntwo words".into(),
            cursor: 1,
            ..Input::default()
        };
        assert!(input.vertical(true));
        assert_eq!(input.cursor, 5);
        assert!(input.vertical(true), "to the end of the last line");
        assert_eq!(input.cursor, input.text.len());
        assert!(!input.vertical(true), "then history may take it");
        assert!(input.vertical(false) && input.vertical(false));
        assert_eq!(input.cursor, 0);
        assert!(!input.vertical(false));
    }

    #[test]
    fn undo_takes_back_a_word_at_a_time_and_redo_puts_it_again() {
        let mut input = Input::default();
        for c in "hello world".chars() {
            input.insert(c.encode_utf8(&mut [0; 4]));
        }
        input.backspace();
        input.backspace();
        assert!(input.undo());
        assert_eq!(input.text, "hello world");
        assert!(input.undo());
        assert_eq!(input.text, "hello ");
        assert!(input.undo() && input.undo());
        assert_eq!(input.text, "");
        assert!(!input.undo());
        assert!(input.redo());
        assert_eq!(input.text, "hello");
        assert_eq!(input.cursor, 5);
        input.insert("!");
        assert!(!input.redo(), "a new edit drops what was undone");
    }

    #[test]
    fn undo_brings_back_a_sent_message_with_its_chips() {
        let mut input = Input::default();
        input.insert("see");
        input.paste(&"x".repeat(PASTE_THRESHOLD + 1));
        input.take();
        assert!(input.undo());
        assert_eq!(input.text.chars().count(), 4);
        assert_eq!(input.pastes.len(), 1);
    }
}
