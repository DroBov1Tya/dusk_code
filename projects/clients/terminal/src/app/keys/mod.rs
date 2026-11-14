//! Keys. Typing goes to the message; Tab walks the foldable entries; panels take every
//! key while open.

mod input;
mod panels;
mod workshop;

use super::{
    Action, App, Focus, Panel,
    mouse::inside,
    panels::{Connection, Preset},
};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use dusk_protocol::client::Command;
use std::time::Duration;

/// What /effort offers, as the core names them.
pub const EFFORTS: [&str; 9] = [
    "default", "off", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
];

/// How long a first Ctrl+C waits for the second.
const QUIT_WINDOW: Duration = Duration::from_secs(5);

impl App {
    pub fn handle(&mut self, event: Event) -> Action {
        match event {
            Event::FocusGained => {
                self.focused = Some(true);
                Action::None
            }
            Event::FocusLost => {
                self.focused = Some(false);
                Action::None
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => self.key(key),
            Event::Paste(text) if matches!(self.panel, Some(Panel::Connect(_))) => {
                if let Some(Panel::Connect(form)) = &mut self.panel {
                    let single_line: String = text.chars().filter(|c| !c.is_control()).collect();
                    if let Some(field) = form.field() {
                        field.insert(&single_line);
                    }
                    form.error = None;
                }
                Action::None
            }
            Event::Paste(text)
                if matches!(self.panel, Some(Panel::Plugins { entry: Some(_), .. })) =>
            {
                if let Some(Panel::Plugins {
                    entry: Some(entry), ..
                }) = &mut self.panel
                {
                    let single_line: String = text.chars().filter(|c| !c.is_control()).collect();
                    entry.input.insert(&single_line);
                    entry.error = None;
                }
                Action::None
            }
            Event::Paste(text) if matches!(self.panel, Some(Panel::Paste(_))) => {
                self.editor_paste(&text.replace("\r\n", "\n").replace('\r', "\n"));
                Action::None
            }
            Event::Mouse(mouse) if matches!(self.panel, Some(Panel::Agent(_))) => {
                self.agent_window_mouse(mouse)
            }
            Event::Mouse(mouse) if matches!(self.panel, Some(Panel::Settings(_))) => {
                self.settings_mouse(mouse)
            }
            Event::Paste(text) if matches!(self.panel, Some(Panel::Settings(_))) => {
                self.settings_paste(&text.replace("\r\n", "\n").replace('\r', "\n"));
                Action::None
            }
            Event::Mouse(mouse) if matches!(self.panel, Some(Panel::Paste(_))) => {
                self.editor_mouse(mouse)
            }
            Event::Mouse(mouse) if matches!(self.panel, Some(Panel::Update { .. })) => {
                let pressed = mouse.kind == MouseEventKind::Down(MouseButton::Left);
                match &self.panel {
                    Some(Panel::Update {
                        info: Some(info),
                        progress: None,
                        button: Some((row, left, right)),
                        ..
                    }) if pressed
                        && mouse.row == *row
                        && (*left..*right).contains(&mouse.column) =>
                    {
                        Action::InstallUpdate(info.latest.clone())
                    }
                    _ => Action::None,
                }
            }
            Event::Mouse(mouse) if matches!(self.panel, Some(Panel::Connect(_))) => {
                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && let Some(Panel::Connect(form)) = &mut self.panel
                    && let Some(index) = form.field_areas.iter().position(|area| {
                        area.is_some_and(|area| inside(area, mouse.column, mouse.row))
                    })
                {
                    match form.preset {
                        // It has no form: the click chooses it, Enter signs in.
                        None if Preset::ALL.get(index) == Some(&Preset::ChatGpt) => {
                            form.choice = index;
                        }
                        None if index < Preset::ALL.len() => form.pick(Preset::ALL[index]),
                        None => {}
                        Some(_) => form.selected = index.min(Connection::FIELDS - 1),
                    }
                }
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

    pub(super) fn key(&mut self, key: KeyEvent) -> Action {
        self.flash = None;
        if self.panel.is_none() && key.code == KeyCode::Esc && self.selection.take().is_some() {
            return Action::None;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let typing = self.panel.is_none() && self.focus == Focus::Input;
        if typing && key.code == KeyCode::Esc && self.input.anchor.take().is_some() {
            return Action::None;
        }
        // With text selected in the box, Ctrl+C copies it instead of clearing the box.
        if typing
            && ctrl
            && key.code == KeyCode::Char('c')
            && let Some(text) = self.input.selected()
        {
            return Action::Copy(text);
        }
        if ctrl && key.code == KeyCode::Char('c') {
            let selected = match &self.panel {
                Some(Panel::Agent(window)) => window.selected_text(),
                None => self.selection.and_then(|selection| {
                    (selection.anchor != selection.cursor)
                        .then(|| self.selected_text(selection))
                        .filter(|text| !text.trim().is_empty())
                }),
                _ => None,
            };
            if let Some(text) = selected {
                self.quit_armed = None;
                return Action::Copy(text);
            }
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
            KeyCode::Char('t') if ctrl => return Action::Command(Command::Resume { cwd: None }),
            KeyCode::Char('n') if ctrl => return Action::Command(Command::New),
            KeyCode::Char('o') if ctrl => return Action::Command(Command::Settings),
            KeyCode::Char('r') if ctrl => {
                self.transcript.toggle_reasoning();
                return Action::None;
            }
            KeyCode::Char('b') if ctrl => {
                match self.transcript.running_command() {
                    Some(call_id) => return Action::Background(call_id),
                    None => self.flash = Some("no command runs in the foreground".into()),
                }
                return Action::None;
            }
            KeyCode::F(3) => {
                if let Some(card) = self.subagents.last() {
                    return Action::OpenAgent(card.thread_id.clone());
                }
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
        if !self.approvals.is_empty() {
            return self.approval_key(key);
        }
        if self.poll.as_ref().is_some_and(|poll| poll.active) {
            return self.poll_key(key);
        }
        match self.focus {
            Focus::Input => self.input_key(key),
            Focus::Browse(selected) => self.browse_key(key, selected),
        }
    }

    pub fn scroll_by(&mut self, lines: usize, up: bool) -> bool {
        let before = self.scroll;
        self.scroll = if up {
            self.scroll.saturating_add(lines).min(self.max_scroll)
        } else {
            self.scroll.saturating_sub(lines)
        };
        self.scroll != before
    }
}
