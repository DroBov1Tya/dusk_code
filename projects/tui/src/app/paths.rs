//! A path typed in the message, after a space or as a word with a second `/`, lists
//! what its folder holds above the box; Tab walks the entries into the text. The core
//! reads the folder; this keeps what it found for the word being typed.

use std::env;

use dusk_core::chat::complete::{self, PathCompletion};

use super::App;

pub struct PathPopup {
    /// Byte offset in the box where the word starts.
    pub start: usize,
    /// The word as typed, before Tab put entries in.
    pub word: String,
    pub found: PathCompletion,
    /// The entry Tab put in last.
    pub selected: Option<usize>,
}

impl App {
    /// Reads the folder again only when the word changed; while Tab walks the entries
    /// the word stays as typed.
    pub fn refresh_paths(&mut self) {
        if self
            .paths
            .as_ref()
            .is_some_and(|popup| popup.selected.is_some())
        {
            return;
        }
        let Some((start, word)) = self.path_word() else {
            self.paths = None;
            return;
        };
        if self.paths.as_ref().is_some_and(|popup| popup.word == word) {
            return;
        }
        let cwd = env::current_dir().unwrap_or_default();
        let found = complete::paths(&word, &cwd);
        self.paths = (!found.entries.is_empty()).then_some(PathPopup {
            start,
            word,
            found,
            selected: None,
        });
    }

    /// The word before the cursor, when it reads as a path. At the start of the box a
    /// single `/word` is a command.
    fn path_word(&self) -> Option<(usize, String)> {
        let before = &self.input.text[..self.input.cursor];
        let start = before.rfind(char::is_whitespace).map_or(0, |at| {
            at + before[at..].chars().next().map_or(1, char::len_utf8)
        });
        let word = &before[start..];
        let command = start == 0 && word.matches('/').count() < 2 && word.starts_with('/');
        (complete::is_path(word) && !command).then(|| (start, word.to_owned()))
    }

    /// Puts the next entry in place of the word. Returns whether Tab was used.
    pub fn cycle_path(&mut self) -> bool {
        let Some(popup) = &mut self.paths else {
            return false;
        };
        let count = popup.found.entries.len();
        let selected = popup.selected.map_or(0, |selected| (selected + 1) % count);
        popup.selected = Some(selected);
        let entry = &popup.found.entries[selected];
        let folder = &popup.word[..=popup.word.rfind('/').unwrap_or(0)];
        let slash = if entry.directory { "/" } else { "" };
        let text = format!("{folder}{}{slash}", entry.name);
        let start = popup.start;
        self.input.anchor = Some(start);
        self.input.insert(&text);
        // A single entry is the answer: the next key reads its folder.
        if count == 1 {
            self.paths = None;
        }
        true
    }

    /// Any key but Tab keeps what Tab put in and lists from there.
    pub fn end_path_cycle(&mut self) {
        if let Some(popup) = &mut self.paths
            && popup.selected.is_some()
        {
            self.paths = None;
        }
    }
}
