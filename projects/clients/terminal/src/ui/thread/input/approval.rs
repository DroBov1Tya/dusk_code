//! The question for a tool's yes, shown over the input.

use super::InputView;
use crate::app::{App, ApprovalHit, Asked};
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

/// Rows of a batch's steps the box grows to; more are paged, never cut.
const MAX_STEP_ROWS: usize = 12;

/// Steps sit under the title, indented past its leading space.
const STEP_INDENT: &str = "   ";

/// The border, the request, a batch's steps, its choices and the hint.
pub(super) fn approval_rows(app: &App, width: u16) -> u16 {
    app.approvals.first().map_or(0, |asked| {
        let steps = step_lines(asked, width.saturating_sub(2) as usize).len();
        let paged = steps > MAX_STEP_ROWS;
        let shown = steps.min(MAX_STEP_ROWS) + usize::from(paged);
        (asked.choices().len() + 4 + shown) as u16
    })
}

/// Every step of a batch, each wrapped whole to the box: the user answers for all of
/// them, so none is clipped. Control characters are drawn as `?`.
fn step_lines(asked: &Asked, width: usize) -> Vec<String> {
    let room = width.saturating_sub(STEP_INDENT.len()).max(1);
    asked
        .request
        .steps
        .iter()
        .flat_map(|step| {
            let line: String = step
                .line()
                .chars()
                .map(|c| if c.is_control() { '?' } else { c })
                .collect();
            wrap::chars(&line, room)
        })
        .map(|row| format!("{STEP_INDENT}{row}"))
        .collect()
}

pub(super) const SEND: &str = " send ";

pub(super) const SKIP: &str = " skip ";

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
        let steps = step_lines(asked, width);
        let choices = asked.choices().len();
        // What is left for steps once the title, choices and hint have their rows.
        let room = (inner.height as usize).saturating_sub(choices + 2);
        let paged = steps.len() > room;
        let page = match paged {
            true => room.saturating_sub(1).max(1),
            false => steps.len(),
        };
        let first = asked.step_scroll.min(steps.len().saturating_sub(page));
        let shown = steps.iter().skip(first).take(page);
        lines.extend(shown.map(|row| Line::from(Span::raw(row.clone()))));
        if paged {
            let last = (first + page).min(steps.len());
            lines.push(Line::from(Span::styled(
                wrap::clip(
                    &format!(
                        "   rows {}-{last} of {}  pgup/pgdn  a yes shows the rest first",
                        first + 1,
                        steps.len()
                    ),
                    width,
                ),
                dim(),
            )));
        }
        let step_rows = lines.len() as u16 - 1;
        let seen = first + page >= steps.len();
        for (row, (decision, label)) in asked.choices().into_iter().enumerate() {
            let here = asked.cursor == row;
            let pointer = if here { " \u{203A} " } else { "   " };
            let style = match (here, decision) {
                (true, Decision::Deny) => bold(NEGATIVE),
                (true, _) => bold(ACCENT),
                (false, _) => Style::new(),
            };
            let y = inner.y + 1 + step_rows + row as u16;
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
        if let Some(asked) = self.app.approvals.first_mut() {
            asked.step_scroll = first;
            asked.step_page = page;
            if seen && asked.steps_seen_at.is_none() {
                asked.steps_seen_at = Some(std::time::Instant::now());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::approval_batch as batch;

    #[test]
    fn every_step_is_drawn_whole_however_long() {
        let text = "x".repeat(150);
        let asked = batch(20, &text);
        let rows = step_lines(&asked, 40);
        assert!(rows.len() > MAX_STEP_ROWS);
        assert!(rows.iter().all(|row| row.chars().count() <= 40));
        let drawn: String = rows.iter().map(|row| &row[STEP_INDENT.len()..]).collect();
        let whole: String = asked.request.steps.iter().map(|step| step.line()).collect();
        assert_eq!(drawn, whole);
    }
}
