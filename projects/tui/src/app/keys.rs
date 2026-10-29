//! Keys and mouse. Typing goes to the message; Tab walks the foldable entries; panels take
//! every key while open. Dragging over the transcript marks text and copies it; a click
//! opens or closes a folded entry or presses a button of the entry under the mouse.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use unicode_width::UnicodeWidthChar;

use super::{
    Action, App, Button, Entry, Focus, Panel, Selection, SettingKind, SettingList, TEXT_COLUMN,
    commands, panels::step,
};

/// How long a first Ctrl+C waits for the second.
const QUIT_WINDOW: Duration = Duration::from_secs(5);

impl App {
    pub fn handle(&mut self, event: Event) -> Action {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.key(key),
            Event::Paste(text) if matches!(self.panel, Some(Panel::Paste(_))) => {
                self.editor_paste(&text.replace("\r\n", "\n").replace('\r', "\n"));
                Action::None
            }
            Event::Mouse(mouse) if matches!(self.panel, Some(Panel::Paste(_))) => {
                self.editor_mouse(mouse);
                Action::None
            }
            Event::Paste(text) if self.panel.is_none() => {
                self.focus = Focus::Input;
                self.input
                    .paste(&text.replace("\r\n", "\n").replace('\r', "\n"));
                Action::None
            }
            Event::Mouse(mouse) if self.panel.is_none() => self.mouse(mouse),
            _ => Action::None,
        }
    }

    fn key(&mut self, key: KeyEvent) -> Action {
        self.flash = None;
        if key.code == KeyCode::Esc && self.selection.take().is_some() {
            return Action::None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            return self.interrupt_key();
        }
        self.quit_armed = None;
        if ctrl && key.code == KeyCode::Char('q') {
            return Action::Quit;
        }
        if self.panel.is_some() {
            return self.panel_key(key);
        }
        match key.code {
            KeyCode::Char('t') if ctrl => return Action::ShowThreads,
            KeyCode::Char('n') if ctrl => return Action::NewThread,
            KeyCode::Char('o') if ctrl => {
                let list = SettingList::load(&self.config.current(), &self.tools, &self.skills_dir);
                self.panel = Some(Panel::Settings(list));
                return Action::None;
            }
            KeyCode::Char('r') if ctrl => {
                self.transcript.toggle_reasoning();
                return Action::None;
            }
            KeyCode::F(1) => {
                self.panel = Some(Panel::Help);
                return Action::None;
            }
            KeyCode::F(2) => {
                if self.compact_layout {
                    self.sidebar_overlay_open = !self.sidebar_overlay_open;
                } else {
                    self.sidebar_visible = !self.sidebar_visible;
                }
                return Action::None;
            }
            KeyCode::PageUp if key.modifiers.contains(KeyModifiers::SHIFT) => {
                let page = self
                    .agent_area
                    .map_or(1, |area| area.height.saturating_sub(2) as usize)
                    .max(1);
                self.agent_scroll = self.agent_scroll.saturating_sub(page);
                return Action::None;
            }
            KeyCode::PageDown if key.modifiers.contains(KeyModifiers::SHIFT) => {
                let page = self
                    .agent_area
                    .map_or(1, |area| area.height.saturating_sub(2) as usize)
                    .max(1);
                self.agent_scroll = self
                    .agent_scroll
                    .saturating_add(page)
                    .min(self.agent_max_scroll);
                return Action::None;
            }
            KeyCode::PageUp if key.modifiers.contains(KeyModifiers::ALT) => {
                let page = self
                    .mcp_area
                    .map_or(1, |area| area.height.saturating_sub(2) as usize)
                    .max(1);
                self.mcp_scroll = self.mcp_scroll.saturating_sub(page);
                return Action::None;
            }
            KeyCode::PageDown if key.modifiers.contains(KeyModifiers::ALT) => {
                let page = self
                    .mcp_area
                    .map_or(1, |area| area.height.saturating_sub(2) as usize)
                    .max(1);
                self.mcp_scroll = self
                    .mcp_scroll
                    .saturating_add(page)
                    .min(self.mcp_max_scroll);
                return Action::None;
            }
            KeyCode::PageUp => {
                if self.scroll_by(self.viewport.saturating_sub(2).max(1), true) {
                    self.focus = Focus::Input;
                }
                return Action::None;
            }
            KeyCode::PageDown => {
                if self.scroll_by(self.viewport.saturating_sub(2).max(1), false) {
                    self.focus = Focus::Input;
                }
                return Action::None;
            }
            _ => {}
        }
        match self.focus {
            Focus::Input => self.input_key(key),
            Focus::Browse(selected) => self.browse_key(key, selected),
        }
    }

    /// Ctrl+C clears the box; a second one within the window quits, so one press
    /// never loses the session.
    fn interrupt_key(&mut self) -> Action {
        if self
            .quit_armed
            .is_some_and(|armed| armed.elapsed() <= QUIT_WINDOW)
        {
            return Action::Quit;
        }
        self.input.take();
        self.quit_armed = Some(Instant::now());
        self.flash = Some("Ctrl+C again to quit".into());
        Action::None
    }

    fn input_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Enter if shift => self.input.insert("\n"),
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
            KeyCode::Char(c) if !ctrl && !alt => self.input.insert(c.encode_utf8(&mut [0; 4])),
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::Left => self.input.left(),
            KeyCode::Right => self.input.right(),
            KeyCode::Home => self.input.home(),
            KeyCode::End => self.input.end(),
            KeyCode::Up if !self.input.vertical(false) => self.recall(true),
            KeyCode::Down if !self.input.vertical(true) => self.recall(false),
            KeyCode::Tab if self.complete_command() => {}
            KeyCode::Tab => {
                if let Some(last) = self.transcript.last_foldable() {
                    self.focus = Focus::Browse(last);
                }
            }
            // Agents can work on after the turn that asked them; Esc stops them too.
            KeyCode::Esc if self.is_busy() || self.agents_at_work() => return Action::Cancel,
            _ => {}
        }
        Action::None
    }

    /// Finishes a command name when only one fits what is typed.
    fn complete_command(&mut self) -> bool {
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

    fn browse_key(&mut self, key: KeyEvent, selected: usize) -> Action {
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
            KeyCode::Esc | KeyCode::Tab => {
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

    fn panel_key(&mut self, key: KeyEvent) -> Action {
        let Some(panel) = &mut self.panel else {
            return Action::None;
        };
        let down = matches!(key.code, KeyCode::Down | KeyCode::Char('j'));
        let up = matches!(key.code, KeyCode::Up | KeyCode::Char('k'));
        match panel {
            Panel::Paste(_) => self.editor_key(key),
            Panel::Help => self.panel = None,
            Panel::Threads(list) => match key.code {
                _ if up || down => {
                    step(&mut list.selected, list.threads.len(), down);
                    let shown = list.preview.as_ref().map(|preview| &preview.thread_id);
                    return match list.threads.get(list.selected) {
                        Some(thread) if shown != Some(&thread.id) => {
                            Action::Preview(thread.id.clone())
                        }
                        _ => Action::None,
                    };
                }
                KeyCode::Enter => {
                    let chosen = list
                        .threads
                        .get(list.selected)
                        .map(|thread| thread.id.clone());
                    self.panel = None;
                    return chosen.map_or(Action::None, Action::OpenThread);
                }
                KeyCode::Char('n') => {
                    self.panel = None;
                    return Action::NewThread;
                }
                KeyCode::Esc => self.panel = None,
                _ => {}
            },
            Panel::Settings(list) => match key.code {
                _ if up || down => step(&mut list.selected, list.items.len(), down),
                KeyCode::Char(' ') | KeyCode::Enter => {
                    let Some(item) = list.items.get_mut(list.selected) else {
                        return Action::None;
                    };
                    let (kind, name, on) = (item.kind, item.name.clone(), !item.on);
                    let saved = self.config.update(|config| match kind {
                        SettingKind::Tool => config.set_tool_enabled(&name, on),
                        SettingKind::Skill => config.set_skill_enabled(&name, on),
                    });
                    match saved {
                        Ok(_) => item.on = on,
                        Err(error) => self.error(format!("cannot save the setting: {error:#}")),
                    }
                }
                KeyCode::Char('r') => {
                    if let Err(error) = self.config.reload() {
                        self.error(format!("cannot reload the config: {error:#}"));
                    }
                    let list =
                        SettingList::load(&self.config.current(), &self.tools, &self.skills_dir);
                    self.panel = Some(Panel::Settings(list));
                }
                KeyCode::Esc => self.panel = None,
                _ => {}
            },
        }
        Action::None
    }

    fn scroll_by(&mut self, lines: usize, up: bool) -> bool {
        let before = self.scroll;
        self.scroll = if up {
            self.scroll.saturating_add(lines).min(self.max_scroll)
        } else {
            self.scroll.saturating_sub(lines)
        };
        self.scroll != before
    }

    fn wheel(&mut self, x: u16, y: u16, up: bool) {
        if self.command_popup.is_some_and(|area| inside(area, x, y)) {
            return;
        }
        if self.agent_area.is_some_and(|area| inside(area, x, y)) {
            self.agent_scroll = if up {
                self.agent_scroll.saturating_sub(3)
            } else {
                self.agent_scroll
                    .saturating_add(3)
                    .min(self.agent_max_scroll)
            };
        } else if self.mcp_area.is_some_and(|area| inside(area, x, y)) {
            self.mcp_scroll = if up {
                self.mcp_scroll.saturating_sub(3)
            } else {
                self.mcp_scroll.saturating_add(3).min(self.mcp_max_scroll)
            };
        } else if self.transcript_area.is_some_and(|area| inside(area, x, y))
            && !self.sidebar_area.is_some_and(|area| inside(area, x, y))
            && self.scroll_by(3, up)
        {
            self.focus = Focus::Input;
        }
    }
}

impl App {
    fn mouse(&mut self, mouse: MouseEvent) -> Action {
        let (x, y) = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::ScrollUp => self.wheel(x, y, true),
            MouseEventKind::ScrollDown => self.wheel(x, y, false),
            MouseEventKind::Moved => self.hover = self.entry_at(x, y),
            MouseEventKind::Down(MouseButton::Left) => {
                self.flash = None;
                if let Some(action) = self.press_button(x, y) {
                    return action;
                }
                self.selection = self.line_at(x, y).map(|at| Selection {
                    anchor: at,
                    cursor: at,
                });
            }
            MouseEventKind::Drag(MouseButton::Left) => self.drag(x, y),
            MouseEventKind::Up(MouseButton::Left) => return self.release(x, y),
            _ => {}
        }
        Action::None
    }

    fn press_button(&mut self, x: u16, y: u16) -> Option<Action> {
        let chip = self
            .input_chips
            .iter()
            .find(|hit| y == hit.row && x >= hit.left && x < hit.right);
        if let Some(index) = chip.map(|hit| hit.index) {
            self.open_paste(index);
            return Some(Action::None);
        }
        let hit = self
            .button_hits
            .iter()
            .find(|hit| y == hit.row && x >= hit.left && x < hit.right)?;
        let (index, button) = (hit.index, hit.button);
        self.selection = None;
        match button {
            Button::Copy => self.transcript.text(index).map(Action::Copy),
            Button::Edit => {
                let Some(Entry::User { text, .. }) = self.transcript.entries.get(index) else {
                    return None;
                };
                let text = text.clone();
                self.input.set(text);
                self.focus = Focus::Input;
                Some(Action::None)
            }
        }
    }

    /// Past the top or bottom edge the transcript scrolls, so a selection can grow.
    fn drag(&mut self, x: u16, y: u16) {
        let (Some(area), Some(mut selection)) = (self.transcript_area, self.selection) else {
            return;
        };
        if y < area.y {
            self.scroll_by(1, true);
        } else if y >= area.bottom() {
            self.scroll_by(1, false);
        }
        let row = y.clamp(area.y, area.bottom().saturating_sub(1));
        let top = self.drawn_top;
        selection.cursor = (top + (row - area.y) as usize, x.max(area.x));
        self.selection = Some(selection);
    }

    /// A press and release in one place is a click; anything else copies what it marked.
    fn release(&mut self, x: u16, y: u16) -> Action {
        let Some(selection) = self.selection else {
            return Action::None;
        };
        if selection.anchor == selection.cursor {
            self.selection = None;
            self.click(x, y);
            return Action::None;
        }
        let text = self.selected_text(selection);
        if text.trim().is_empty() {
            self.selection = None;
            return Action::None;
        }
        Action::Copy(text)
    }

    fn click(&mut self, x: u16, y: u16) {
        let index = self
            .transcript_hits
            .iter()
            .find(|hit| y == hit.row && x >= hit.left && x < hit.right)
            .map(|hit| hit.index);
        if let Some(index) = index {
            self.transcript.toggle(index);
            self.focus = Focus::Browse(index);
        }
    }

    /// The transcript line and column under the mouse, unless something covers it.
    fn line_at(&self, x: u16, y: u16) -> Option<(usize, u16)> {
        let area = self.transcript_area?;
        let covered = [self.command_popup, self.sidebar_overlay]
            .into_iter()
            .flatten()
            .any(|over| inside(over, x, y));
        if covered || !inside(area, x, y) {
            return None;
        }
        Some((self.drawn_top + (y - area.y) as usize, x))
    }

    fn entry_at(&self, x: u16, y: u16) -> Option<usize> {
        let (line, _) = self.line_at(x, y)?;
        self.entry_lines
            .iter()
            .position(|&(from, to)| line >= from && line < to)
    }

    /// What a selection covers, line by line, without the bars and indents on the left.
    fn selected_text(&self, selection: Selection) -> String {
        let left = self.transcript_area.map_or(0, |area| area.x);
        let (start, end) = selection.ordered();
        let mut lines = Vec::new();
        for line in start.0..=end.0 {
            let Some(text) = self.drawn_text.get(line) else {
                break;
            };
            let from = if line == start.0 { start.1 - left } else { 0 };
            let to = if line == end.0 {
                end.1 - left + 1
            } else {
                u16::MAX
            };
            let from = from.max(TEXT_COLUMN);
            lines.push(
                columns(text, from as usize, to as usize)
                    .trim_end()
                    .to_owned(),
            );
        }
        lines.join("\n")
    }
}

/// The part of `text` drawn in columns `from..to`; a wide character goes with its first column.
fn columns(text: &str, from: usize, to: usize) -> String {
    let mut column = 0;
    let mut part = String::new();
    for c in text.chars() {
        if column >= from && column < to {
            part.push(c);
        }
        column += c.width().unwrap_or(0);
    }
    part
}

fn inside(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}
