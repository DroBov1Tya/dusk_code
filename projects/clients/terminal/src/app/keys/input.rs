//! Keys of the message box and the command list.

use super::QUIT_WINDOW;
use crate::app::{Action, App, Focus, Panel, commands};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

impl App {
    /// Ctrl+C clears the box; a second one within the window quits, so one press
    /// never loses the session.
    pub(super) fn interrupt_key(&mut self) -> Action {
        if self
            .quit_armed
            .is_some_and(|armed| armed.elapsed() <= QUIT_WINDOW)
        {
            return Action::Quit;
        }
        if matches!(self.panel, Some(Panel::Connect(_))) {
            self.panel = None;
        }
        self.input.take();
        self.quit_armed = Some(Instant::now());
        self.flash = Some("Ctrl+C again to quit".into());
        Action::None
    }

    pub(super) fn input_key(&mut self, key: KeyEvent) -> Action {
        if key.code != KeyCode::Tab {
            self.input.command_cycle = None;
            self.end_path_cycle();
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Enter if shift => self.input.insert("\n"),
            // Where Shift+Enter reads as Enter, a backslash before it asks for a new line.
            KeyCode::Enter if self.input.text[..self.input.cursor].ends_with('\\') => {
                self.input.backspace();
                self.input.insert("\n");
            }
            // A command typed in part runs as the one it can only be.
            KeyCode::Enter => {
                self.complete_command();
                return self.submit(alt);
            }
            KeyCode::Char('j') if ctrl => self.input.insert("\n"),
            KeyCode::Char('u') if ctrl => {
                self.input.take();
            }
            KeyCode::Char('e') if ctrl => {
                if let Some(index) = self.input.chip_at_cursor() {
                    self.open_paste(index);
                }
            }
            KeyCode::Char('a') if ctrl => self.input.select_all(),
            KeyCode::Char('x') if ctrl => {
                if let Some(text) = self.input.selected() {
                    self.input.delete_selection();
                    return Action::Copy(text);
                }
            }
            KeyCode::Char(c) if !ctrl && !alt => self.input.insert(c.encode_utf8(&mut [0; 4])),
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End => {
                self.input.mark(shift);
                match key.code {
                    KeyCode::Left => self.input.left(),
                    KeyCode::Right => self.input.right(),
                    KeyCode::Home => self.input.home(),
                    _ => self.input.end(),
                }
            }
            KeyCode::Up | KeyCode::Down if shift => {
                self.input.mark(true);
                self.input.vertical(key.code == KeyCode::Down);
            }
            KeyCode::Up | KeyCode::Down => {
                self.input.mark(false);
                let down = key.code == KeyCode::Down;
                if !self.input.vertical(down) {
                    self.recall(!down);
                }
            }
            KeyCode::Tab if self.cycle_path() => {}
            KeyCode::Tab if self.cycle_command() => {}
            KeyCode::Tab if self.poll.is_some() => {
                if let Some(poll) = &mut self.poll {
                    poll.active = true;
                }
            }
            KeyCode::Tab => return Action::NextMode,
            KeyCode::BackTab => {
                if let Some(last) = self.transcript.last_foldable() {
                    self.focus = Focus::Browse(last);
                }
            }
            // Agents can work on after the turn that asked them; Esc stops them too.
            KeyCode::Esc if self.is_busy() || self.agents_at_work() => {
                self.unqueue_into_box();
                return Action::Cancel;
            }
            _ => {}
        }
        Action::None
    }

    /// Tab selects a candidate without submitting or changing the operating mode.
    pub(super) fn cycle_command(&mut self) -> bool {
        let typed = self.input.text.as_str();
        if !typed.starts_with('/') || typed.contains(char::is_whitespace) {
            self.input.command_cycle = None;
            return false;
        }
        let (prefix, previous) = match &self.input.command_cycle {
            Some((prefix, selected)) => (prefix.clone(), Some(*selected)),
            None => (typed.to_owned(), None),
        };
        let fitting: Vec<_> = commands()
            .filter(|(name, _)| name.starts_with(&prefix))
            .collect();
        if !fitting.is_empty() {
            let selected = previous.map_or(0, |selected| (selected + 1) % fitting.len());
            self.input.set(fitting[selected].0.to_owned());
            self.input.command_cycle = Some((prefix, selected));
        }
        // Even an unknown slash token owns Tab until the command token ends.
        true
    }

    /// Finishes a command name when only one fits what is typed.
    pub(super) fn complete_command(&mut self) -> bool {
        let typed = self.input.text.as_str();
        if !typed.starts_with('/') || typed.contains(char::is_whitespace) {
            return false;
        }
        let mut fitting = commands().filter(|(name, _)| name.starts_with(typed));
        match (fitting.next(), fitting.next()) {
            (Some((name, _)), None) => {
                self.input.set(format!("{name} "));
                true
            }
            _ => false,
        }
    }

    pub(super) fn browse_key(&mut self, key: KeyEvent, selected: usize) -> Action {
        let transcript = &self.transcript;
        let moved = match key.code {
            KeyCode::Up | KeyCode::Char('k') => transcript.next_foldable(selected, false),
            KeyCode::Down | KeyCode::Char('j') => transcript.next_foldable(selected, true),
            KeyCode::Home | KeyCode::Char('g') => transcript.next_foldable(0, true),
            KeyCode::End | KeyCode::Char('G') => transcript.last_foldable(),
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.transcript.toggle(selected);
                None
            }
            KeyCode::Esc | KeyCode::BackTab => {
                self.focus = Focus::Input;
                None
            }
            KeyCode::Char(c) => {
                self.focus = Focus::Input;
                self.input.insert(c.encode_utf8(&mut [0; 4]));
                None
            }
            _ => None,
        };
        if let Some(index) = moved {
            self.focus = Focus::Browse(index);
        }
        Action::None
    }
}
