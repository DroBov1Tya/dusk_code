//! The question for a tool's yes, shown over the input.

use super::InputView;
use crate::app::{App, ApprovalHit};
use crate::ui::thread::{
    theme::{ACCENT, DUSK, NEGATIVE, TEXT, bold, dim, fg},
    wrap,
};
use dusk_protocol::tools::Decision;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};

/// The border, the request, its choices and the hint.
pub(super) fn approval_rows(app: &App) -> u16 {
    app.approvals
        .first()
        .map_or(0, |asked| asked.choices().len() as u16 + 4)
}

pub(super) const SEND: &str = " send ";

pub(super) const DECLINE: &str = " decline ";

impl InputView<'_> {
    /// The first request of a tool waiting for the user's yes, with its choices.
    pub(super) fn draw_approval(&mut self, frame: &mut Frame, area: Rect) {
        self.app.approval_hits.clear();
        let Some(asked) = self.app.approvals.first() else {
            return;
        };
        if area.height < 4 || area.width < 20 {
            return;
        }
        let waiting = self.app.approvals.len();
        let title = match waiting {
            1 => " approval ".to_owned(),
            _ => format!(" approval 1 of {waiting} "),
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(ACCENT))
            .title(Span::styled(title, bold(DUSK)));
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        let width = inner.width as usize;
        let mut lines = vec![Line::from(Span::styled(
            wrap::clip(&format!(" {}", asked.title()), width),
            bold(TEXT),
        ))];
        for (row, (decision, label)) in asked.choices().into_iter().enumerate() {
            let here = asked.cursor == row;
            let pointer = if here { " \u{203A} " } else { "   " };
            let style = match (here, decision) {
                (true, Decision::Deny) => bold(NEGATIVE),
                (true, _) => bold(ACCENT),
                (false, _) => Style::new(),
            };
            let y = inner.y + 1 + row as u16;
            if y < inner.bottom() {
                self.app.approval_hits.push(ApprovalHit {
                    row: y,
                    left: inner.x,
                    right: inner.right(),
                    choice: row,
                });
            }
            lines.push(Line::from(Span::styled(
                wrap::clip(&format!("{pointer}{label}"), width),
                style,
            )));
        }
        lines.push(Line::from(Span::styled(
            "  up/down choose  enter answers  esc denies",
            dim(),
        )));
        frame.render_widget(Paragraph::new(lines), inner);
    }
}
