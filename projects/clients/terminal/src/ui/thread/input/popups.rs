//! The lists that open above the input: paths and commands.

use super::InputView;
use crate::app::commands;
use crate::ui::thread::theme::{ACCENT, HAIRLINE, RAISED, TEXT, bold, dim, fg};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

/// The command list above the box while a command is being typed.
/// Rows of a folder listing above the box; more wait behind the last one.
pub(super) const PATH_ROWS: usize = 8;

impl InputView<'_> {
    /// What the folder of the path being typed holds, folders first.
    pub fn draw_paths(&mut self, frame: &mut Frame, above: Rect) {
        let Some(popup) = &self.app.paths else {
            return;
        };
        let entries = &popup.found.entries;
        let selected = popup.selected.unwrap_or(0);
        let skip = (selected + 1).saturating_sub(PATH_ROWS);
        let more = entries.len().saturating_sub(skip + PATH_ROWS);
        let mut lines: Vec<Line> = entries
            .iter()
            .enumerate()
            .skip(skip)
            .take(PATH_ROWS)
            .map(|(index, entry)| {
                let chosen = popup.selected == Some(index);
                let slash = if entry.directory { "/" } else { "" };
                let style = match (chosen, entry.directory) {
                    (true, _) => bold(ACCENT).bg(RAISED),
                    (false, true) => bold(TEXT),
                    (false, false) => Style::new(),
                };
                Line::from(Span::styled(format!("  {}{slash}  ", entry.name), style))
            })
            .collect();
        if more > 0 || popup.found.partial {
            let note = match (popup.found.partial, more) {
                (true, 0) => "  large folder; type to narrow  ".to_owned(),
                (true, _) => format!("  {more}+ more; type to narrow  "),
                (false, _) => format!("  {more} more  "),
            };
            lines.push(Line::from(Span::styled(note, dim())));
        }
        let width = lines
            .iter()
            .map(Line::width)
            .max()
            .unwrap_or(0)
            .max(24)
            .min(above.width.saturating_sub(2) as usize) as u16
            + 2;
        let height = (lines.len() as u16 + 2).min(above.height);
        let area = Rect {
            x: above.x + 1,
            y: above.bottom().saturating_sub(height),
            width,
            height,
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(HAIRLINE))
            .title(Span::styled(" tab completes ", dim()));
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }

    pub fn draw_commands(&mut self, frame: &mut Frame, above: Rect) {
        self.app.command_popup = None;
        let text = self
            .app
            .input
            .command_cycle
            .as_ref()
            .map_or(self.app.input.text.as_str(), |(prefix, _)| prefix.as_str());
        if !text.starts_with('/') || text.contains(char::is_whitespace) {
            return;
        }
        let matching: Vec<_> = commands()
            .filter(|(name, _)| name.starts_with(text))
            .collect();
        if matching.is_empty() {
            return;
        }
        let width = matching
            .iter()
            .map(|(name, about)| name.width() + about.width() + 6)
            .max()
            .unwrap_or(0)
            .min(above.width.saturating_sub(1) as usize) as u16;
        let height = matching.len() as u16 + 2;
        let area = Rect {
            x: above.x + 1,
            y: above.bottom().saturating_sub(height),
            width,
            height: height.min(above.height),
        };
        self.app.command_popup = Some(area);
        let selected = self
            .app
            .input
            .command_cycle
            .as_ref()
            .map_or(0, |(_, selected)| *selected);
        let visible = area.height.saturating_sub(2) as usize;
        let skip = (selected + 1).saturating_sub(visible);
        let lines: Vec<Line> = matching
            .iter()
            .enumerate()
            .skip(skip)
            .take(visible)
            .map(|(index, (name, about))| {
                let selected = self
                    .app
                    .input
                    .command_cycle
                    .as_ref()
                    .is_some_and(|(_, selected)| *selected == index);
                Line::from(vec![
                    Span::styled(
                        format!(" {}{name}", if selected { "> " } else { "  " }),
                        if selected {
                            bold(ACCENT).bg(RAISED)
                        } else {
                            bold(TEXT)
                        },
                    ),
                    Span::styled(format!("  {about}"), dim()),
                ])
            })
            .collect();
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(HAIRLINE));
        frame.render_widget(Clear, area);
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }
}
