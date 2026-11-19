//! The question the agent asks, shown over the input.

use super::InputView;
use super::approval::{DECLINE, SEND, SKIP};
use super::layout::{MAX_OWN_LINES, OWN_LEAD, own_lines};
use crate::app::{App, PollHit, PollTarget};
use crate::ui::text::format::printable;
use crate::ui::thread::{
    theme::{ACCENT, DOT, DUSK, HAIRLINE, NEGATIVE, RAISED, TEXT, TEXT_SOFT, bold, dim, fg},
    wrap,
};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

/// The question's border, its text, the options, the own answer and the buttons.
pub(super) fn poll_rows(app: &App, width: u16) -> u16 {
    // A light question is the reply row's, not a card.
    let card = app.poll.as_ref().filter(|poll| !poll.question.light());
    card.map_or(0, |poll| {
        let own = own_lines(&poll.own.text, width.saturating_sub(2) as usize).len();
        (poll.question.options.len() + own) as u16 + 4
    })
}

/// The reply row: one row, or none.
pub(super) fn reply_rows(app: &App) -> u16 {
    u16::from(!app.reply_labels().is_empty())
}

/// What a keypress does, said last and only when it fits.
const REPLY_HINT: &str = "Alt+number";

impl InputView<'_> {
    /// `reply  [1] Yes  [2] No`: labels the agent offered, answered with Alt+number. Plain
    /// and unfilled, so it does not pass for a button or a prompt of the app, and the
    /// agent's own words are cut to the row.
    pub(super) fn draw_reply(&self, frame: &mut Frame, area: Rect) {
        let labels = self.app.reply_labels();
        if area.height == 0 || labels.is_empty() {
            return;
        }
        let width = area.width as usize;
        let cursor = self
            .app
            .poll
            .as_ref()
            .filter(|poll| poll.active)
            .map(|poll| poll.cursor);
        let long = format!(" reply {DOT} from the agent");
        let caption = if long.width() + 2 + 4 <= width {
            long
        } else {
            " reply".to_owned()
        };
        let mut used = caption.width();
        let mut spans = vec![Span::styled(caption, dim())];
        for (at, label) in labels.iter().enumerate() {
            let number = format!("  [{}] ", at + 1);
            let room = width.saturating_sub(used + number.width());
            if room < 2 {
                break;
            }
            let label = wrap::clip(&printable(label), room);
            let style = match cursor == Some(at) {
                true => bold(TEXT).add_modifier(Modifier::UNDERLINED),
                false => fg(TEXT_SOFT),
            };
            used += number.width() + label.width();
            spans.push(Span::styled(number, dim()));
            spans.push(Span::styled(label, style));
        }
        let hint = match self.app.poll {
            Some(_) => format!("  {REPLY_HINT}  Ctrl+S skips"),
            None => format!("  {REPLY_HINT}"),
        };
        if used + hint.width() <= width {
            spans.push(Span::styled(hint, dim()));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

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
            "space marks  enter sends  ctrl+s skips  tab to the box  esc declines"
        } else {
            "enter sends  ctrl+s skips  tab to the box  esc declines"
        };
        let buttons_row = inner.y + lines.len() as u16;
        let send_left = inner.x + 1;
        let skip_left = send_left + SEND.width() as u16 + 1;
        let decline_left = skip_left + SKIP.width() as u16 + 1;
        if buttons_row < inner.bottom() {
            poll.hits.push(PollHit {
                row: buttons_row,
                left: send_left,
                right: send_left + SEND.width() as u16,
                target: PollTarget::Send,
            });
            poll.hits.push(PollHit {
                row: buttons_row,
                left: skip_left,
                right: skip_left + SKIP.width() as u16,
                target: PollTarget::Skip,
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
            Span::styled(SKIP, fg(TEXT_SOFT).bg(RAISED)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;
    use dusk_protocol::tools::Question;
    use ratatui::{Terminal, backend::TestBackend};

    /// The reply row drawn `width` columns wide, as text.
    fn row(app: &mut App, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).expect("test terminal");
        terminal
            .draw(|frame| InputView { app }.draw_reply(frame, frame.area()))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        (0..width).map(|x| buffer[(x, 0)].symbol()).collect()
    }

    fn light(options: &[&str]) -> App {
        let mut app = test_app();
        app.ask_question(Question {
            question: "Which?".into(),
            options: options.iter().map(|option| option.to_string()).collect(),
            multiple: false,
        });
        app
    }

    #[test]
    fn the_row_numbers_the_labels_and_says_whose_they_are() {
        let mut app = light(&["Yes", "No", "Later"]);
        assert_eq!(reply_rows(&app), 1);
        assert_eq!(poll_rows(&app, 80), 0, "a light question has no card");
        let drawn = row(&mut app, 80);
        assert!(drawn.contains("reply"), "{drawn}");
        assert!(drawn.contains("from the agent"), "{drawn}");
        assert!(drawn.contains("[1] Yes  [2] No  [3] Later"), "{drawn}");
        assert!(drawn.contains("Alt+number"), "{drawn}");
        assert!(drawn.contains("Ctrl+S skips"), "{drawn}");
    }

    #[test]
    fn a_question_that_is_not_light_has_its_card_and_no_row() {
        let app = light(&["a", "b", "c", "d", "e"]);
        assert_eq!(reply_rows(&app), 0);
        assert!(poll_rows(&app, 80) > 0);
    }

    #[test]
    fn hostile_labels_cannot_steer_the_terminal_or_overflow_the_row() {
        let wide = "\u{6570}".repeat(12);
        for width in [12, 30, 60, 100] {
            let mut app = light(&[
                "ok\x1b[2J\x1b]0;x\x07",
                "admin\u{202e}txt",
                &wide,
                "\u{1f680}",
            ]);
            let drawn = row(&mut app, width);
            assert!(!drawn.contains(['\x1b', '\x07', '\u{202e}']), "{drawn}");
            // Wide characters fill two cells, so a row too long would lose its tail.
            assert_eq!(drawn.contains("[4]"), width == 100, "{width}: {drawn}");
        }
    }
}
