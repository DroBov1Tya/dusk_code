//! Messages waiting for the turn to end.

use super::InputView;
use crate::app::{App, QueueButton, QueueHit, paste};
use crate::ui::thread::{
    theme::{ACCENT, HAIRLINE, NEGATIVE, RAISED, fg, thought},
    wrap,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

/// Queued messages shown above the box; older ones wait unseen.
pub(super) const MAX_QUEUED: usize = 4;

pub(super) fn queue_rows(app: &App) -> u16 {
    app.queued.len().min(MAX_QUEUED) as u16
}

impl InputView<'_> {
    pub(super) fn draw_queue(&mut self, frame: &mut Frame, area: Rect) {
        self.app.queue_hits.clear();
        let skip = self.app.queued.len().saturating_sub(area.height as usize);
        const SEND: &str = " send now ";
        const REMOVE: &str = " \u{00D7} ";
        let buttons = SEND.width() + REMOVE.width() + 1;
        let mut lines = Vec::new();
        for (index, text) in self.app.queued.iter().enumerate().skip(skip) {
            let row = area.y + lines.len() as u16;
            let label = format!("   \u{29D7} {}", paste::labelled(text).replace('\n', " "));
            let room = (area.width as usize).saturating_sub(buttons);
            let label = wrap::clip(&label, room);
            let gap = room.saturating_sub(label.width());
            let send_left = area.x + (label.width() + gap) as u16;
            let remove_left = send_left + SEND.width() as u16 + 1;
            let hits = [
                (area.x, send_left, QueueButton::Edit),
                (
                    send_left,
                    send_left + SEND.width() as u16,
                    QueueButton::SendNow,
                ),
                (
                    remove_left,
                    remove_left + REMOVE.width() as u16,
                    QueueButton::Remove,
                ),
            ];
            for (left, right, button) in hits {
                self.app.queue_hits.push(QueueHit {
                    row,
                    left,
                    right,
                    index,
                    button,
                });
            }
            lines.push(Line::from(vec![
                Span::styled(label, thought()),
                Span::raw(" ".repeat(gap)),
                Span::styled(SEND, fg(ACCENT).bg(HAIRLINE)),
                Span::raw(" "),
                Span::styled(REMOVE, fg(NEGATIVE).bg(HAIRLINE)),
            ]));
        }
        frame.render_widget(Paragraph::new(lines).style(Style::new().bg(RAISED)), area);
    }
}
