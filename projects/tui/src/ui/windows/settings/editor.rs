//! The editor of a skill, an agent or an MCP server, and the metadata above it.

use super::SettingsWindow;
use super::lines::indented;
use crate::app::{FileButton, FileEditor, HitTarget, SettingsHit};
use crate::ui::windows::{
    input,
    theme::{ACCENT, DOT, DUSK, HAIRLINE, NEGATIVE, RAISED, TEXT_SOFT, bold, dim, fg},
    wrap,
};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph},
};
use unicode_width::UnicodeWidthStr;

/// Rows the front matter box may take at most.
pub(super) const METADATA_ROWS: u16 = 8;

/// The description in full, then every other plain field on a line of its own.
pub(super) fn metadata(editor: &FileEditor, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some((_, description)) = editor.metadata.iter().find(|(key, _)| key == "description") {
        lines.extend(
            wrap::words(vec![Span::raw(description.clone())], width, 0)
                .into_iter()
                .map(Line::from),
        );
    }
    for (key, value) in &editor.metadata {
        if key == "description" || key == "name" {
            continue;
        }
        lines.push(Line::from(vec![
            Span::styled(format!("{key} {DOT} "), fg(TEXT_SOFT)),
            Span::styled(
                wrap::clip(value, width.saturating_sub(key.len() + 3)),
                dim(),
            ),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "no front matter: the file must start with --- and a description",
            fg(NEGATIVE),
        )));
    }
    lines
}

impl SettingsWindow<'_> {
    /// The open skill: its front matter as fields, then the file to edit, then the buttons.
    pub(super) fn editor(&mut self, frame: &mut Frame, area: Rect) {
        let Some(editor) = &mut self.view.editor else {
            return;
        };
        let width = area.width.saturating_sub(2) as usize;
        let fields = metadata(editor, width);
        let top = (fields.len() as u16 + 2)
            .min(METADATA_ROWS)
            .min(area.height / 2);
        let mut title = vec![Span::styled(format!(" {} ", editor.title()), bold(DUSK))];
        if editor.changed() {
            title.push(Span::styled("changed ", fg(ACCENT)));
        }
        let header = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(HAIRLINE))
            .title(Line::from(title));
        frame.render_widget(
            Paragraph::new(fields).block(header),
            Rect {
                height: top,
                ..area
            },
        );
        if area.height <= top + 2 {
            return;
        }
        // An error longer than the room beside the buttons takes rows of its own above them.
        let error_lines = match &editor.error {
            Some(error) if error.width() > area.width.saturating_sub(46) as usize => {
                indented(error, fg(NEGATIVE), " ", area.width as usize)
            }
            _ => Vec::new(),
        };
        let error_rows = (error_lines.len() as u16).min(area.height.saturating_sub(top + 3) / 2);
        let text_area = Rect {
            y: area.y + top,
            height: area.height - top - 1 - error_rows,
            ..area
        };
        if error_rows > 0 {
            frame.render_widget(
                Paragraph::new(error_lines),
                Rect {
                    y: text_area.bottom(),
                    height: error_rows,
                    ..area
                },
            );
        }
        let (rows, (column, row)) = input::layout(&editor.text, text_area.width as usize);
        let height = text_area.height as usize;
        if row < editor.scroll {
            editor.scroll = row;
        } else if row >= editor.scroll + height {
            editor.scroll = row + 1 - height;
        }
        let lines: Vec<Line> = rows
            .into_iter()
            .skip(editor.scroll)
            .take(height)
            .map(|row| Line::from(row.spans))
            .collect();
        frame.render_widget(Paragraph::new(lines), text_area);
        let y = text_area.y + (row - editor.scroll) as u16;
        if self.cursor {
            frame.set_cursor_position(Position::new(text_area.x + column as u16, y));
        }
        let bar = area.bottom() - 1;
        if let Some(error) = editor.error.as_ref().filter(|_| error_rows == 0) {
            let room = area.width.saturating_sub(46) as usize;
            let line = Line::from(Span::styled(wrap::clip(error, room), fg(NEGATIVE)));
            frame.render_widget(Paragraph::new(line), Rect::new(area.x, bar, area.width, 1));
        }
        let buttons = [
            (FileButton::Save, " save ", ACCENT),
            (FileButton::Revert, " revert ", ACCENT),
            (FileButton::Close, " close ", TEXT_SOFT),
        ];
        let mut right = area.right();
        for &(button, label, color) in buttons.iter().rev() {
            let width = label.width() as u16;
            let Some(left) = right.checked_sub(width + 1).filter(|&left| left >= area.x) else {
                break;
            };
            let shown = Line::from(Span::styled(label, bold(color).bg(RAISED)));
            frame.render_widget(Paragraph::new(shown), Rect::new(left, bar, width, 1));
            self.view.hits.insert(
                0,
                SettingsHit {
                    row: bar,
                    left,
                    right: left + width,
                    target: HitTarget::Button(button),
                },
            );
            right = left;
        }
    }
}
