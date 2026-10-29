//! Keys and mouse. Typing goes to the message; Tab walks the foldable entries; panels take
//! every key while open.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use dusk_core::commands::COMMANDS;
use ratatui::layout::Rect;

use super::{Action, App, Focus, Panel, SettingKind, SettingList, panels::step};

impl App {
    pub fn handle(&mut self, event: Event) -> Action {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.key(key),
            Event::Paste(text) if self.panel.is_none() => {
                self.focus = Focus::Input;
                self.input
                    .insert(&text.replace("\r\n", "\n").replace('\r', "\n"));
                Action::None
            }
            Event::Mouse(mouse) => {
                match mouse.kind {
                    MouseEventKind::ScrollUp if self.panel.is_none() => {
                        self.wheel(mouse.column, mouse.row, true)
                    }
                    MouseEventKind::ScrollDown if self.panel.is_none() => {
                        self.wheel(mouse.column, mouse.row, false)
                    }
                    MouseEventKind::Down(MouseButton::Left) if self.panel.is_none() => {
                        let covered = self
                            .command_popup
                            .is_some_and(|area| inside(area, mouse.column, mouse.row))
                            || self
                                .sidebar_overlay
                                .is_some_and(|area| inside(area, mouse.column, mouse.row));
                        if !covered
                            && let Some(index) = self
                                .transcript_hits
                                .iter()
                                .find(|hit| {
                                    mouse.row == hit.row
                                        && mouse.column >= hit.left
                                        && mouse.column < hit.right
                                })
                                .map(|hit| hit.index)
                        {
                            self.transcript.toggle(index);
                            self.focus = Focus::Browse(index);
                        }
                    }
                    _ => {}
                }
                Action::None
            }
            _ => Action::None,
        }
    }

    fn key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c' | 'q')) {
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

    fn input_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Enter if alt => return self.submit(true),
            KeyCode::Enter if shift => self.input.insert("\n"),
            KeyCode::Enter => return self.submit(false),
            KeyCode::Char('j') if ctrl => self.input.insert("\n"),
            KeyCode::Char('u') if ctrl => {
                self.input.take();
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
            KeyCode::Esc if self.is_busy() => return Action::Cancel,
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
        let mut fitting = COMMANDS.iter().filter(|(name, _)| name.starts_with(typed));
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
            Panel::Help => self.panel = None,
            Panel::Threads(list) => match key.code {
                _ if up || down => step(&mut list.selected, list.threads.len(), down),
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

fn inside(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}
