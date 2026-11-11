//! The settings window. Agents are drawn as a circuit: the chat is the source pad, each
//! agent a pad on the trace from the one that may ask it, lit while it can be asked.
//! Skills open beside the list: what their front matter says on top, the raw file below.

mod editor;
mod lines;
mod rows;

use super::{
    panels::window,
    theme::{DUSK, HAIRLINE, RAISED, bold, dim, fg},
};
use crate::app::{App, HitTarget, Panel, SettingCategory, SettingsHit, SettingsView};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

/// A list item: its lines, and where its switch is on the first of its own lines, as
/// columns from the row's start. The first `lead` lines head a group and are not part
/// of the item.
struct Row {
    lines: Vec<Line<'static>>,
    switch: Option<(u16, u16)>,
    lead: usize,
}

/// The settings window: its category list, the rows of each category and the file editor.
pub struct SettingsWindow<'a> {
    pub view: &'a mut SettingsView,
    /// Off while the idle screen plays over the window.
    pub cursor: bool,
}

pub fn draw(frame: &mut Frame, screen: Rect, app: &mut App) {
    let cursor = app.screensaver.is_none();
    let Some(Panel::Settings(view)) = &mut app.panel else {
        return;
    };
    SettingsWindow { view, cursor }.draw(frame, screen);
}

impl SettingsWindow<'_> {
    pub(super) fn hints(&self) -> &'static str {
        match (self.view.category, &self.view.editor) {
            (None, _) => " up/down choose  enter open  esc close ",
            (Some(SettingCategory::Models), _) if self.view.manual_context.is_some() => {
                " digits enter token count  Ctrl+A select all  enter save  esc cancel "
            }
            (Some(SettingCategory::VersionControl), _) if self.view.branch_input.is_some() => {
                " type the prefix  Ctrl+A select all  enter save  esc cancel "
            }
            (Some(SettingCategory::VersionControl), _) => " enter/click edit  esc back ",
            (Some(SettingCategory::System), _) => " read every 2 s while open  esc back ",
            (Some(SettingCategory::Limits | SettingCategory::Appearance), _) => {
                " up/down choose  left/right change  esc back "
            }
            (Some(SettingCategory::Models), _) => {
                " up/down choose  enter/click apply  manual opens token count  esc back "
            }
            (Some(SettingCategory::Personalize), _) => {
                " up/down choose  space/click switch  left/right or click +/- size  esc back "
            }
            (
                Some(SettingCategory::Tools | SettingCategory::Updates | SettingCategory::Access),
                _,
            ) => " up/down choose  space/click switch  esc back ",
            (Some(_), Some(_)) => " Ctrl+S save  Ctrl+R revert  Esc close  click another one ",
            (Some(SettingCategory::Agents), None) => {
                " up/down choose  enter edit  space switch  left/right limit  r reload  esc back "
            }
            (Some(SettingCategory::Skills), None) => {
                " up/down choose  enter open  space switch  r reload  esc back "
            }
            (Some(SettingCategory::Mcp), None) => {
                " up/down choose  enter edit  t tools  n new  c reconnect  l sign in  space switch  esc back "
            }
        }
    }

    /// The items of the list, scrolled so the selected one shows whole, with their hits.
    pub(super) fn list(&mut self, frame: &mut Frame, area: Rect) {
        let rows = self.rows(area.width as usize);
        let height = area.height as usize;
        let shown = |from: usize, to: usize| -> usize {
            rows[from..=to.min(rows.len().saturating_sub(1))]
                .iter()
                .map(|row| row.lines.len())
                .sum()
        };
        self.view.top = self.view.top.min(self.view.selected);
        while self.view.top < self.view.selected
            && shown(self.view.top, self.view.selected) > height
        {
            self.view.top += 1;
        }
        let mut lines = Vec::new();
        for (index, row) in rows.into_iter().enumerate().skip(self.view.top) {
            if lines.len() >= height {
                break;
            }
            let selectable = index < self.view.len();
            let first = area.y + (lines.len() + row.lead) as u16;
            if self.view.category == Some(SettingCategory::Personalize) && matches!(index, 1 | 3) {
                let (label, chars) = if index == 1 {
                    ("  memory size, characters", self.view.memory.max_chars)
                } else {
                    (
                        "  project memory size, characters",
                        self.view.work_history.max_chars,
                    )
                };
                let end = format!("− {chars} +  ");
                let start = area
                    .width
                    .saturating_sub(end.width() as u16)
                    .max(label.width() as u16);
                for (offset, more) in [(0, false), (end.width() as u16 - 3, true)] {
                    let left = area.x.saturating_add(start).saturating_add(offset);
                    if left < area.right() {
                        self.view.hits.insert(
                            0,
                            SettingsHit {
                                row: first,
                                left,
                                right: (left + 1).min(area.right()),
                                target: HitTarget::MemorySize { row: index, more },
                            },
                        );
                    }
                }
            }
            if let Some((from, to)) = row.switch {
                // First, so a click on the switch wins over the row under it.
                self.view.hits.insert(
                    0,
                    SettingsHit {
                        row: first,
                        left: area.x + from,
                        right: area.x + to,
                        target: HitTarget::Switch(index),
                    },
                );
            }
            for (at, line) in row.lines.into_iter().enumerate() {
                if lines.len() >= height {
                    break;
                }
                if at < row.lead {
                    lines.push(line);
                    continue;
                }
                if selectable {
                    self.view.hits.push(SettingsHit {
                        row: area.y + lines.len() as u16,
                        left: area.x,
                        right: area.right(),
                        target: HitTarget::Row(index),
                    });
                }
                lines.push(if selectable && index == self.view.selected {
                    line.style(Style::new().bg(RAISED))
                } else {
                    line
                });
            }
        }
        frame.render_widget(Paragraph::new(lines), area);
    }

    pub fn draw(&mut self, frame: &mut Frame, screen: Rect) {
        self.view.hits.clear();
        let area = window(screen, 150);
        let title = match self.view.category {
            None => " settings ".to_owned(),
            Some(SettingCategory::Tools) => {
                format!(" settings / tools in {} mode ", self.view.mode)
            }
            Some(category) => format!(" settings / {} ", category.name()),
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(HAIRLINE))
            .title(Line::from(Span::styled(title, bold(DUSK))))
            .title_bottom(Line::from(Span::styled(self.hints(), dim())).right_aligned());
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        if inner.width < 10 || inner.height < 3 {
            return;
        }
        let editable = matches!(
            self.view.category,
            Some(SettingCategory::Agents | SettingCategory::Skills | SettingCategory::Mcp)
        );
        if self.view.editor.is_none() || !editable {
            return self.list(frame, inner);
        }
        let list_width = (inner.width * 3 / 10).max(24).min(inner.width / 2);
        self.list(
            frame,
            Rect {
                width: list_width,
                ..inner
            },
        );
        let divider = Rect::new(inner.x + list_width, inner.y, 1, inner.height);
        let line: Vec<Line> = (0..inner.height)
            .map(|_| Line::from(Span::styled("\u{2502}", fg(HAIRLINE))))
            .collect();
        frame.render_widget(Paragraph::new(line), divider);
        let pane = Rect {
            x: inner.x + list_width + 2,
            width: inner.width.saturating_sub(list_width + 2),
            ..inner
        };
        self.editor(frame, pane);
    }
}
