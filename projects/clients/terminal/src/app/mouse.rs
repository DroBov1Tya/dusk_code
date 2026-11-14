//! The mouse. Dragging over the transcript marks text and copies it; a click opens or
//! closes a folded entry or presses a button of the entry under the mouse.

use std::time::Instant;

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{buffer::CellWidth, layout::Rect, style::Style, text::Span};

use super::{Action, App, Button, Entry, Focus, Pressed, Selection, TEXT_COLUMN};

impl App {
    pub fn mouse(&mut self, mouse: MouseEvent) -> Action {
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
                if let Some(task_id) = self.task_at(x, y) {
                    match self.unfolded_tasks.iter().position(|id| *id == task_id) {
                        Some(index) => {
                            self.unfolded_tasks.remove(index);
                        }
                        None => self.unfolded_tasks.push(task_id),
                    }
                    return Action::None;
                }
                if let Some(thread_id) = self.card_at(x, y) {
                    return Action::OpenAgent(thread_id);
                }
                // Folded, the whole section unfolds; unfolded, its top border folds it.
                if let Some(area) = self.mcp_area.filter(|area| inside(*area, x, y))
                    && (!self.connectors_open || y == area.y)
                {
                    self.connectors_open = !self.connectors_open;
                    return Action::None;
                }
                if self.input_area.is_some_and(|area| inside(area, x, y)) {
                    self.selection = None;
                    self.focus = Focus::Input;
                    if let Some(poll) = &mut self.poll {
                        poll.active = false;
                    }
                    self.input_dragging = true;
                    if let Some(offset) = self.input_offset_at(x, y) {
                        self.input.place(offset);
                    }
                    self.input.anchor = Some(self.input.cursor);
                    return Action::None;
                }
                self.selection = self.line_at(x, y).map(|at| Selection {
                    anchor: at,
                    cursor: at,
                });
            }
            MouseEventKind::Drag(MouseButton::Left) if self.input_dragging => {
                if let Some(offset) = self.input_offset_at(x, y) {
                    self.input.place(offset);
                }
            }
            MouseEventKind::Up(MouseButton::Left) if self.input_dragging => {
                self.input_dragging = false;
                match self.input.selected() {
                    Some(text) => return Action::Copy(text),
                    None => self.input.anchor = None,
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => self.drag(x, y),
            MouseEventKind::Up(MouseButton::Left) => return self.release(x, y),
            _ => {}
        }
        Action::None
    }

    /// A pressed button shows it for a moment: its cells darken.
    fn press(&mut self, (row, left, right): (u16, u16, u16)) {
        self.pressed = Some(Pressed {
            row,
            left,
            right,
            at: Instant::now(),
        });
    }

    fn press_button(&mut self, x: u16, y: u16) -> Option<Action> {
        let approved = self
            .approval_hits
            .iter()
            .find(|hit| y == hit.row && x >= hit.left && x < hit.right)
            .map(|hit| (hit.choice, (hit.row, hit.left, hit.right)));
        if let Some((choice, place)) = approved {
            self.press(place);
            return Some(self.answer_approval(Some(choice)));
        }
        let polled = self.poll.as_ref().and_then(|poll| {
            poll.hits
                .iter()
                .find(|hit| y == hit.row && x >= hit.left && x < hit.right)
                .map(|hit| (hit.target, (hit.row, hit.left, hit.right)))
        });
        if let Some((target, place)) = polled {
            self.press(place);
            return Some(self.press_poll(target));
        }
        let goal = self
            .goal_hits
            .iter()
            .find(|hit| y == hit.row && x >= hit.left && x < hit.right)
            .map(|hit| (hit.button, (hit.row, hit.left, hit.right)));
        if let Some((button, place)) = goal {
            self.press(place);
            return Some(self.press_goal(button));
        }
        let queued = self
            .queue_hits
            .iter()
            .find(|hit| y == hit.row && x >= hit.left && x < hit.right)
            .map(|hit| (hit.index, hit.button, (hit.row, hit.left, hit.right)));
        if let Some((index, button, place)) = queued {
            self.press(place);
            return Some(self.press_queued(index, button));
        }
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
        self.press((hit.row, hit.left, hit.right));
        self.selection = None;
        match button {
            Button::Copy => self.transcript.text(index).map(Action::Copy),
            Button::Hide => {
                self.transcript.hide(index);
                self.hover = None;
                Some(Action::None)
            }
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

    /// The byte offset in the box under the mouse; above or below the shown rows it
    /// goes to the first or last one.
    fn input_offset_at(&self, x: u16, y: u16) -> Option<usize> {
        let first = self.input_rows.first()?;
        let last = self.input_rows.last()?;
        let row = if y < first.row {
            first
        } else {
            self.input_rows
                .iter()
                .find(|row| row.row == y)
                .unwrap_or(last)
        };
        let before = row.cells.iter().rev().find(|&&(left, _)| left <= x);
        let offset = match before {
            _ if x >= row.right => row.end,
            Some(&(_, offset)) => offset,
            None => row.cells.first().map_or(row.end, |&(_, offset)| offset),
        };
        Some(offset)
    }

    fn task_at(&self, x: u16, y: u16) -> Option<String> {
        self.task_hits
            .iter()
            .find(|(area, _)| inside(*area, x, y))
            .map(|(_, task_id)| task_id.clone())
    }

    /// The subagent whose card is under the mouse. Cards take three rows and a blank one
    /// after, below the section's top border.
    fn card_at(&self, x: u16, y: u16) -> Option<String> {
        let area = self.agent_area.filter(|area| inside(*area, x, y))?;
        // The scroll hint occupies the last row instead of agent content.
        if self.agent_max_scroll > 0 && area.height > 2 && y == area.bottom() - 1 {
            return None;
        }
        let row = (y - area.y).checked_sub(1)? as usize + self.agent_scroll;
        if row % 4 == 3 {
            return None;
        }
        let card = self.subagents.get(row / 4)?;
        Some(card.thread_id.clone())
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
    pub fn selected_text(&self, selection: Selection) -> String {
        selected_lines(
            selection,
            self.transcript_area.map_or(0, |area| area.x),
            |line| self.drawn_text(line),
        )
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
        } else if self.task_area.is_some_and(|area| inside(area, x, y)) {
            self.task_scroll = if up {
                self.task_scroll.saturating_sub(3)
            } else {
                self.task_scroll.saturating_add(3).min(self.task_max_scroll)
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

/// Shared column-based selection for the main transcript and agent windows.
pub fn selected_lines<'a>(
    selection: Selection,
    left: u16,
    text_at: impl Fn(usize) -> Option<&'a str>,
) -> String {
    let (start, end) = selection.ordered();
    let mut lines = Vec::new();
    for line in start.0..=end.0 {
        let Some(text) = text_at(line) else { break };
        let from = if line == start.0 {
            start.1.saturating_sub(left)
        } else {
            0
        };
        let to = if line == end.0 {
            end.1.saturating_sub(left).saturating_add(1)
        } else {
            u16::MAX
        };
        lines.push(
            columns(text, from.max(TEXT_COLUMN) as usize, to as usize)
                .trim_end()
                .to_owned(),
        );
    }
    lines.join("\n")
}

/// The part of `text` drawn in columns `from..to`; a wide grapheme goes with its first column.
fn columns(text: &str, from: usize, to: usize) -> String {
    let mut column = 0;
    let mut part = String::new();
    // Use Ratatui's graphemes so combining marks and joined emoji stay with the
    // displayed cell they belong to, including at the right selection boundary.
    let span = Span::raw(text);
    for grapheme in span.styled_graphemes(Style::default()) {
        if column >= from && column < to {
            part.push_str(grapheme.symbol);
        }
        column += usize::from(grapheme.symbol.cell_width());
    }
    part
}

pub fn inside(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}
