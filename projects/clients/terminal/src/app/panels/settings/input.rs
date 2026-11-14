//! What the keys, the paste and the mouse do in the settings panel.

use super::types::{FileButton, HitTarget, McpRow, SettingCategory};
use super::view::{LIMIT_ROWS, PAGE, WHEEL};
use crate::app::panels::{Panel, step};
use crate::app::{Action, App, Input};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dusk_protocol::{client::Command, config::ContextPolicy};

impl App {
    pub fn settings_key(&mut self, key: KeyEvent) -> Action {
        let Some(Panel::Settings(view)) = &mut self.panel else {
            return Action::None;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(input) = &mut view.branch_input {
            match key.code {
                KeyCode::Esc => view.branch_input = None,
                KeyCode::Enter => return Action::SetBranchPrefix(input.text.trim().to_owned()),
                KeyCode::Char('a') if ctrl => input.select_all(),
                KeyCode::Char(c) if !ctrl && c.is_ascii_graphic() => {
                    input.insert(c.encode_utf8(&mut [0; 4]))
                }
                KeyCode::Backspace => input.backspace(),
                KeyCode::Delete => input.delete(),
                KeyCode::Left => {
                    input.mark(false);
                    input.left();
                }
                KeyCode::Right => {
                    input.mark(false);
                    input.right();
                }
                KeyCode::Home => {
                    input.mark(false);
                    input.home();
                }
                KeyCode::End => {
                    input.mark(false);
                    input.end();
                }
                _ => {}
            }
            return Action::None;
        }
        if let Some(input) = &mut view.manual_context {
            match key.code {
                KeyCode::Esc => {
                    view.manual_context = None;
                    view.model_error = None;
                }
                KeyCode::Enter => match input.text.parse::<u64>().ok().filter(|tokens| *tokens > 0)
                {
                    Some(tokens) => return Action::SetContextPolicy(ContextPolicy::Manual(tokens)),
                    None => {
                        view.model_error =
                            Some("enter a token count above zero (digits only)".into())
                    }
                },
                KeyCode::Char('a') if ctrl => input.select_all(),
                KeyCode::Char(c) if c.is_ascii_digit() && !ctrl => {
                    input.insert(c.encode_utf8(&mut [0; 4]));
                    view.model_error = None;
                }
                KeyCode::Backspace => input.backspace(),
                KeyCode::Delete => input.delete(),
                KeyCode::Left => {
                    input.mark(false);
                    input.left();
                }
                KeyCode::Right => {
                    input.mark(false);
                    input.right();
                }
                KeyCode::Home => {
                    input.mark(false);
                    input.home();
                }
                KeyCode::End => {
                    input.mark(false);
                    input.end();
                }
                _ => {}
            }
            return Action::None;
        }
        if let Some(editor) = &mut view.editor {
            let text = &mut editor.text;
            match key.code {
                KeyCode::Char('s') if ctrl => return view.press(FileButton::Save),
                KeyCode::Char('r') if ctrl => return view.press(FileButton::Revert),
                KeyCode::Esc => return view.press(FileButton::Close),
                KeyCode::Char(c) if !ctrl => text.insert(c.encode_utf8(&mut [0; 4])),
                KeyCode::Enter => text.insert("\n"),
                KeyCode::Tab => text.insert("  "),
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
            editor.error = None;
            return Action::None;
        }
        let up = matches!(key.code, KeyCode::Up) || (ctrl && key.code == KeyCode::Char('p'));
        let down = matches!(key.code, KeyCode::Down) || (ctrl && key.code == KeyCode::Char('n'));
        match (view.category, key.code) {
            _ if up || down => {
                let count = view.len();
                step(&mut view.selected, count, down);
            }
            (Some(SettingCategory::Models), KeyCode::Enter | KeyCode::Char(' '))
                if view.selected == 5 =>
            {
                view.edit_manual_context()
            }
            (Some(SettingCategory::VersionControl), KeyCode::Enter) => view.edit_branch_prefix(),
            (Some(SettingCategory::Models), KeyCode::Enter | KeyCode::Char(' ')) => {
                return view.model_action(view.selected).unwrap_or(Action::None);
            }
            (None, KeyCode::Esc) => self.panel = None,
            (None, KeyCode::Enter | KeyCode::Right) => {
                view.category = SettingCategory::ALL.get(view.selected).copied();
                view.selected = 0;
                view.top = 0;
            }
            (Some(category), KeyCode::Esc | KeyCode::Left | KeyCode::Backspace)
                if !(key.code == KeyCode::Left
                    && (category == SettingCategory::Limits
                        || category == SettingCategory::Appearance
                        || category == SettingCategory::Agents && view.selected < LIMIT_ROWS
                        || category == SettingCategory::Personalize
                            && matches!(view.selected, 1 | 3))) =>
            {
                view.selected = SettingCategory::ALL
                    .iter()
                    .position(|item| *item == category)
                    .unwrap_or(0);
                view.category = None;
                view.top = 0;
            }
            (
                Some(SettingCategory::Agents),
                KeyCode::Left | KeyCode::Right | KeyCode::Char('-' | '+'),
            ) if view.selected < LIMIT_ROWS => {
                let more = matches!(key.code, KeyCode::Right | KeyCode::Char('+'));
                if view.selected == 1 {
                    let rounds = if more {
                        view.max_goal_rounds.saturating_add(1)
                    } else {
                        view.max_goal_rounds.saturating_sub(1).max(1)
                    };
                    return Action::SetGoalRounds(rounds);
                }
                let limit = if more {
                    view.max_agents_at_work.saturating_add(1)
                } else {
                    view.max_agents_at_work.saturating_sub(1).max(1)
                };
                return Action::SetAgentLimit(limit);
            }
            (
                Some(SettingCategory::Appearance),
                KeyCode::Left | KeyCode::Right | KeyCode::Char('-' | '+'),
            ) => {
                let minutes = if matches!(key.code, KeyCode::Right | KeyCode::Char('+')) {
                    view.idle_screen_minutes.saturating_add(1)
                } else {
                    view.idle_screen_minutes.saturating_sub(1)
                };
                return Action::SetIdleMinutes(minutes);
            }
            (
                Some(SettingCategory::Limits),
                KeyCode::Left | KeyCode::Right | KeyCode::Char('-' | '+'),
            ) => {
                let more = matches!(key.code, KeyCode::Right | KeyCode::Char('+'));
                return view.limit_action(more);
            }
            (
                Some(SettingCategory::Personalize),
                KeyCode::Left | KeyCode::Right | KeyCode::Char('-' | '+'),
            ) if matches!(view.selected, 1 | 3) => {
                let more = matches!(key.code, KeyCode::Right | KeyCode::Char('+'));
                return view.memory_size_action(view.selected, more);
            }
            (Some(SettingCategory::Agents), KeyCode::Enter)
                if view.agent_file(view.selected).is_some() =>
            {
                if let Some(name) = view.agent_file(view.selected) {
                    return Action::OpenAgentFile(name);
                }
            }
            (Some(SettingCategory::Skills), KeyCode::Enter) => {
                if let Some(skill) = view.skills.get(view.selected) {
                    return Action::OpenSkill(skill.name.clone());
                }
            }
            (Some(SettingCategory::Mcp), KeyCode::Enter) => {
                match view.mcp_rows().get(view.selected) {
                    Some(McpRow::Server(server)) => {
                        return Action::OpenMcp(Some(server.name.clone()));
                    }
                    Some(McpRow::Tool(..)) => {
                        return view.switch(view.selected).unwrap_or(Action::None);
                    }
                    None => {}
                }
            }
            (Some(SettingCategory::Mcp), KeyCode::Char('t')) => {
                view.toggle_mcp_tools(view.selected)
            }
            (Some(SettingCategory::Mcp), KeyCode::Char('n')) => return Action::OpenMcp(None),
            (Some(SettingCategory::Mcp), KeyCode::Char('c')) => {
                if let Some(server) = view.mcp_server(view.selected) {
                    return Action::RestartMcp(server.name.clone());
                }
            }
            (Some(SettingCategory::Mcp), KeyCode::Char('l')) => {
                if let Some(server) = view.mcp_server(view.selected) {
                    return Action::SignInMcp(server.name.clone());
                }
            }
            (Some(_), KeyCode::Char(' ') | KeyCode::Enter) => {
                if let Some(action) = view.switch(view.selected) {
                    return action;
                }
            }
            (Some(_), KeyCode::Char('r')) => return Action::Command(Command::Settings),
            _ => {}
        }
        Action::None
    }

    pub fn settings_paste(&mut self, pasted: &str) {
        if let Some(Panel::Settings(view)) = &mut self.panel
            && let Some(input) = &mut view.branch_input
        {
            input.insert(
                &pasted
                    .chars()
                    .filter(char::is_ascii_graphic)
                    .collect::<String>(),
            );
            return;
        }
        if let Some(Panel::Settings(view)) = &mut self.panel
            && let Some(input) = &mut view.manual_context
        {
            if pasted.chars().all(|c| c.is_ascii_digit()) {
                input.insert(pasted);
                view.model_error = None;
            } else {
                view.model_error = Some("enter digits only".into());
            }
            return;
        }
        if let Some(Panel::Settings(view)) = &mut self.panel
            && let Some(editor) = &mut view.editor
        {
            editor.text.insert(pasted);
        }
    }

    pub fn settings_mouse(&mut self, mouse: MouseEvent) -> Action {
        let Some(Panel::Settings(view)) = &mut self.panel else {
            return Action::None;
        };
        let (x, y) = (mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                if view.manual_context.is_some() {
                    return Action::None;
                }
                let down = mouse.kind == MouseEventKind::ScrollDown;
                match &mut view.editor {
                    Some(editor) => move_rows(&mut editor.text, WHEEL, down),
                    None => {
                        let count = view.len();
                        for _ in 0..WHEEL {
                            step(&mut view.selected, count, down);
                        }
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let hit = view
                    .hits
                    .iter()
                    .find(|hit| y == hit.row && x >= hit.left && x < hit.right)
                    .map(|hit| hit.target);
                match hit {
                    Some(HitTarget::Button(button)) => return view.press(button),
                    Some(HitTarget::MemorySize { row, more }) => {
                        view.selected = row;
                        return view.memory_size_action(row, more);
                    }
                    Some(HitTarget::Switch(row)) => {
                        view.selected = row;
                        return view.switch(row).unwrap_or(Action::None);
                    }
                    Some(HitTarget::Row(row)) => return view.click_row(row),
                    None => {}
                }
            }
            _ => {}
        }
        Action::None
    }
}

pub(super) fn move_rows(text: &mut Input, rows: usize, down: bool) {
    for _ in 0..rows {
        if !text.vertical(down) {
            break;
        }
    }
}
