//! The bottom of the screen: a rule that tells what the agent is doing and how full the
//! context is, then the message being typed, on a background of its own. Typing `/`
//! lists the commands.

mod approval;
mod footer;
mod goal;
mod layout;
mod poll;
mod popups;
mod queue;

use approval::approval_rows;
use footer::status;
use goal::goal_rows;
pub use layout::layout;
use layout::{show_cursor, text_width};
use poll::poll_rows;
use queue::queue_rows;

use super::theme::{ACCENT, HAIRLINE, SELECTED, SURFACE, TEXT_SOFT, bold, fg, thought};
use crate::app::{App, ChipHit, InputRow};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph},
};
use unicode_width::UnicodeWidthStr;

const MIN_ROWS: usize = 3;

const MAX_ROWS: usize = 10;

const PROMPT: &str = " \u{203A} ";

/// Rows around the text: a blank row to breathe, then the card's two edges.
const FRAME_ROWS: u16 = 3;

/// Columns between the card's edge and its text on each side: the edge and one blank.
const SIDE: u16 = 2;

/// The message box and what sits above it: approvals, polls, the goal, the queue and the popups.
pub struct InputView<'a> {
    pub app: &'a mut App,
}

/// Rows the box needs: the queued messages, the rule, the padding and the wrapped text.
pub fn height(app: &App, width: u16) -> u16 {
    let (rows, _) = layout(&app.input, text_width(width));
    approval_rows(app)
        + poll_rows(app, width)
        + goal_rows(app)
        + queue_rows(app)
        + rows.len().clamp(MIN_ROWS, MAX_ROWS) as u16
        + FRAME_ROWS
        + 1
}

impl InputView<'_> {
    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let approvals = self.app.status.approvals;
        self.draw_footer(
            frame,
            area,
            self.app.git.as_ref(),
            &self.app.folder,
            approvals,
        );
        let area = Rect {
            height: area.height.saturating_sub(1),
            ..area
        };
        let approval = approval_rows(self.app).min(area.height);
        self.draw_approval(
            frame,
            Rect {
                height: approval,
                ..area
            },
        );
        let area = Rect {
            y: area.y + approval,
            height: area.height.saturating_sub(approval),
            ..area
        };
        let polled = poll_rows(self.app, area.width).min(area.height);
        self.draw_poll(
            frame,
            Rect {
                height: polled,
                ..area
            },
        );
        let area = Rect {
            y: area.y + polled,
            height: area.height.saturating_sub(polled),
            ..area
        };
        let goal = goal_rows(self.app).min(area.height);
        self.draw_goal(
            frame,
            Rect {
                height: goal,
                ..area
            },
        );
        let area = Rect {
            y: area.y + goal,
            height: area.height.saturating_sub(goal),
            ..area
        };
        let queued = queue_rows(self.app).min(area.height);
        self.draw_queue(
            frame,
            Rect {
                height: queued,
                ..area
            },
        );
        let area = Rect {
            y: area.y + queued,
            height: area.height.saturating_sub(queued),
            ..area
        };
        // The message is a card: what the agent does on its top edge, what is below on the
        // right of it, and the text inside with a column of air on each side.
        let box_area = Rect {
            y: area.y + 1,
            height: area.height.saturating_sub(1),
            ..area
        };
        let (doing, below) = status(self.app);
        let edge = if self.app.is_busy() {
            SELECTED
        } else {
            HAIRLINE
        };
        let mut card = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(fg(edge))
            .style(Style::new().bg(SURFACE))
            .title(Line::from(doing));
        if !below.is_empty() {
            card = card
                .title(Line::from(Span::styled(format!(" {below} "), fg(ACCENT))).right_aligned());
        }
        if let Some(tag) = &self.app.update_available {
            let note = format!(" {tag} is out \u{00B7} /update ");
            card = card.title_bottom(Line::from(Span::styled(note, fg(TEXT_SOFT))).right_aligned());
        }
        frame.render_widget(card, box_area);
        let text_area = Rect {
            x: box_area.x + SIDE,
            y: box_area.y + 1,
            width: box_area.width.saturating_sub(2 * SIDE),
            height: box_area.height.saturating_sub(2),
        };
        self.app.input_chips.clear();
        self.app.input_rows.clear();
        self.app.input_area = Some(box_area);
        let width = text_width(area.width);
        let prompt = Span::styled(PROMPT, bold(ACCENT));
        if self.app.input.text.is_empty() {
            let hint = if !self.app.provider_available {
                "provider unavailable; /connect or /model to choose a provider"
            } else if self.app.is_busy() {
                "write to queue it; Alt+Enter sends now, Esc stops"
            } else {
                "write, or / for commands"
            };
            let line = Line::from(vec![prompt, Span::styled(hint, thought())]);
            frame.render_widget(Paragraph::new(line), text_area);
            show_cursor(frame, self.app, text_area, (0, 0));
            return;
        }
        let (rows, cursor) = layout(&self.app.input, width);
        let skip = (cursor.1 + 1).saturating_sub(text_area.height as usize);
        let left = text_area.x + PROMPT.width() as u16;
        let mut lines = Vec::new();
        for (row, shown) in rows.into_iter().enumerate().skip(skip) {
            let y = text_area.y + (row - skip) as u16;
            self.app.input_rows.push(InputRow {
                row: y,
                right: left + shown.width as u16,
                cells: shown
                    .cells
                    .iter()
                    .map(|&(column, offset)| (left + column as u16, offset))
                    .collect(),
                end: shown.end,
            });
            for &(from, to, index) in &shown.chips {
                self.app.input_chips.push(ChipHit {
                    row: y,
                    left: left + from as u16,
                    right: left + to as u16,
                    index,
                });
            }
            let lead = if row == 0 {
                prompt.clone()
            } else {
                Span::raw("   ")
            };
            lines.push(Line::from([vec![lead], shown.spans].concat()));
        }
        frame.render_widget(Paragraph::new(lines), text_area);
        show_cursor(frame, self.app, text_area, (cursor.0, cursor.1 - skip));
    }
}
