//! A pasted text open in a window of its own. Ctrl+S saves it back into its chip; closing
//! with changes asks whether to save them, throw them away or go on editing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::{App, Input, Panel};

pub struct PasteEditor {
    /// The chip the text belongs to.
    pub index: usize,
    pub text: Input,
    original: String,
    /// Closing was asked with unsaved changes.
    pub confirming: bool,
    /// The first row shown, moved so the cursor stays in view.
    pub scroll: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditorButton {
    Save,
    Close,
    Discard,
    KeepEditing,
}

/// A button of the editor on the last drawn frame.
pub struct EditorHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub button: EditorButton,
}

/// Rows the cursor moves for PgUp, PgDn and the wheel.
const PAGE: usize = 20;
const WHEEL: usize = 3;

impl PasteEditor {
    pub fn new(index: usize, text: String) -> Self {
        Self {
            index,
            text: Input {
                text: text.clone(),
                cursor: 0,
                pastes: Vec::new(),
                anchor: None,
            },
            original: text,
            confirming: false,
            scroll: 0,
        }
    }

    pub fn changed(&self) -> bool {
        self.text.text != self.original
    }
}

impl App {
    /// Opens the pasted text of a chip in the box.
    pub fn open_paste(&mut self, index: usize) {
        if let Some(text) = self.input.pastes.get(index) {
            self.panel = Some(Panel::Paste(PasteEditor::new(index, text.clone())));
        }
    }

    pub fn editor_key(&mut self, key: KeyEvent) {
        let Some(Panel::Paste(editor)) = &mut self.panel else {
            return;
        };
        if editor.confirming {
            let button = match key.code {
                KeyCode::Char('y') | KeyCode::Enter => EditorButton::Save,
                KeyCode::Char('n') => EditorButton::Discard,
                KeyCode::Esc | KeyCode::Char('c') => EditorButton::KeepEditing,
                _ => return,
            };
            return self.press_editor(button);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let text = &mut editor.text;
        match key.code {
            KeyCode::Char('s') if ctrl => self.press_editor(EditorButton::Save),
            KeyCode::Esc => self.press_editor(EditorButton::Close),
            KeyCode::Char(c) if !ctrl => text.insert(c.encode_utf8(&mut [0; 4])),
            KeyCode::Enter => text.insert("\n"),
            KeyCode::Tab => text.insert("    "),
            KeyCode::Backspace => text.backspace(),
            KeyCode::Delete => text.delete(),
            KeyCode::Left => text.left(),
            KeyCode::Right => text.right(),
            KeyCode::Home => text.home(),
            KeyCode::End => text.end(),
            KeyCode::Up => {
                text.vertical(false);
            }
            KeyCode::Down => {
                text.vertical(true);
            }
            KeyCode::PageUp => move_rows(text, PAGE, false),
            KeyCode::PageDown => move_rows(text, PAGE, true),
            _ => {}
        }
    }

    /// Pasting into the editor types the text in; it is a chip already.
    pub fn editor_paste(&mut self, pasted: &str) {
        if let Some(Panel::Paste(editor)) = &mut self.panel
            && !editor.confirming
        {
            editor.text.insert(pasted);
        }
    }

    pub fn editor_mouse(&mut self, mouse: MouseEvent) {
        let Some(Panel::Paste(editor)) = &mut self.panel else {
            return;
        };
        match mouse.kind {
            MouseEventKind::ScrollUp => move_rows(&mut editor.text, WHEEL, false),
            MouseEventKind::ScrollDown => move_rows(&mut editor.text, WHEEL, true),
            MouseEventKind::Down(MouseButton::Left) => {
                let (x, y) = (mouse.column, mouse.row);
                let hit = self
                    .editor_hits
                    .iter()
                    .find(|hit| y == hit.row && x >= hit.left && x < hit.right);
                if let Some(button) = hit.map(|hit| hit.button) {
                    self.press_editor(button);
                }
            }
            _ => {}
        }
    }

    fn press_editor(&mut self, button: EditorButton) {
        let Some(Panel::Paste(editor)) = &mut self.panel else {
            return;
        };
        match button {
            EditorButton::Close if editor.changed() => editor.confirming = true,
            EditorButton::KeepEditing => editor.confirming = false,
            EditorButton::Close | EditorButton::Discard => self.panel = None,
            EditorButton::Save => {
                let (index, text) = (editor.index, editor.text.text.clone());
                self.panel = None;
                self.input.replace_paste(index, text);
            }
        }
    }
}

fn move_rows(text: &mut Input, rows: usize, down: bool) {
    for _ in 0..rows {
        if !text.vertical(down) {
            break;
        }
    }
}
