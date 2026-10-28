//! The message being typed: text with a cursor, several lines allowed.

#[derive(Default)]
pub struct Input {
    pub text: String,
    /// Byte offset into `text`, always on a character boundary.
    pub cursor: usize,
}

impl Input {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    pub fn set(&mut self, text: String) {
        self.cursor = text.len();
        self.text = text;
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
