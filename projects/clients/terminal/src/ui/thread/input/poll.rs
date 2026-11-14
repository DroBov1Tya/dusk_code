//! The question the agent asks, shown over the input.

use super::InputView;
use super::approval::{DECLINE, SEND};
use super::layout::{MAX_OWN_LINES, OWN_LEAD, own_lines};
use crate::app::{App, PollHit, PollTarget};
use crate::ui::thread::{
    theme::{ACCENT, DUSK, HAIRLINE, NEGATIVE, RAISED, TEXT, bold, dim, fg},
    wrap,
};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

/// The question's border, its text, the options, the own answer and the buttons.
pub(super) fn poll_rows(app: &App, width: u16) -> u16 {
    app.poll.as_ref().map_or(0, |poll| {
        let own = own_lines(&poll.own.text, width.saturating_sub(2) as usize).len();
        (poll.question.options.len() + own) as u16 + 4
    })
}

impl InputView<'_> {
    /// The question the agent asked: options to choose, the user's own answer, and buttons.
    pub(super) fn draw_poll(&mut self, frame: &mut Frame, area: Rect) {
        let cursor_allowed = self.app.screensaver.is_none();
        let Some(poll) = &mut self.app.poll else {
            return;
        };
        poll.hits.clear();
        if area.height < 4 || area.width < 20 {
            return;
        }
        let border = if poll.active { ACCENT } else { HAIRLINE };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(border))
            .title(Span::styled(" question ", bold(DUSK)));
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        let width = inner.width as usize;
        let mut lines = vec![Line::from(Span::styled(
            wrap::clip(&format!(" {}", poll.question.question), width),
            bold(TEXT),
        ))];
        let own_row = poll.own_row();
        let visible_rows = inner.height.saturating_sub(1) as usize;
        for row in (0..=own_row).take(visible_rows) {
            let here = poll.active && poll.cursor == row;
            let pointer = if here { " \u{203A} " } else { "   " };
            let style = if here { bold(ACCENT) } else { Style::new() };
            let labels = if row < own_row {
                let mark = match (poll.question.multiple, poll.checked[row]) {
                    (true, true) => "[x] ",
                    (true, false) => "[ ] ",
                    (false, _) if here => "(\u{2022}) ",
                    (false, _) => "( ) ",
                };
                let option = &poll.question.options[row];
                vec![wrap::clip(&format!("{pointer}{mark}{option}"), width)]
            } else if poll.own.text.is_empty() && !here {
                vec![format!("{pointer}\u{270E}  your own answer")]
            } else {
                // The first line carries the pencil, the rest line up under it.
                own_lines(&poll.own.text, width)
                    .into_iter()
                    .enumerate()
                    .map(|(at, line)| match at {
                        0 => format!("{pointer}\u{270E}  {line}"),
                        _ => format!("{}{line}", " ".repeat(OWN_LEAD)),
                    })
                    .collect()
            };
            for label in labels {
                poll.hits.push(PollHit {
                    row: inner.y + lines.len() as u16,
                    left: inner.x,
                    right: inner.right(),
                    target: PollTarget::Row(row),
                });
                lines.push(Line::from(Span::styled(label, style)));
            }
        }
        let hint = if poll.question.multiple {
            "space marks  enter sends  tab to the box  esc declines"
        } else {
            "enter sends  tab to the box  esc declines"
        };
        let buttons_row = inner.y + lines.len() as u16;
        let send_left = inner.x + 1;
        let decline_left = send_left + SEND.width() as u16 + 1;
        if buttons_row < inner.bottom() {
            poll.hits.push(PollHit {
                row: buttons_row,
                left: send_left,
                right: send_left + SEND.width() as u16,
                target: PollTarget::Send,
            });
            poll.hits.push(PollHit {
                row: buttons_row,
                left: decline_left,
                right: decline_left + DECLINE.width() as u16,
                target: PollTarget::Decline,
            });
        }
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(SEND, fg(ACCENT).bg(RAISED)),
            Span::raw(" "),
            Span::styled(DECLINE, fg(NEGATIVE).bg(RAISED)),
            Span::styled(format!("  {hint}"), dim()),
        ]));
        frame.render_widget(Paragraph::new(lines), inner);
        if poll.active && poll.cursor == own_row && own_row < visible_rows && cursor_allowed {
            // Wrapping by characters puts the text before the cursor on the same lines.
            let all = wrap::chars(&poll.own.text, width.saturating_sub(OWN_LEAD)).len();
            let before = wrap::chars(
                &poll.own.text[..poll.own.cursor],
                width.saturating_sub(OWN_LEAD),
            );
            let shown_from = all.saturating_sub(MAX_OWN_LINES);
            let line = (before.len() - 1).saturating_sub(shown_from);
            let column = OWN_LEAD + before.last().map_or(0, |last| last.width());
            let x = (inner.x as usize + column).min(inner.right().saturating_sub(1) as usize);
            let y = inner.y + 1 + (own_row + line) as u16;
            if y < inner.bottom() {
                frame.set_cursor_position(Position::new(x as u16, y));
            }
        }
    }
}
