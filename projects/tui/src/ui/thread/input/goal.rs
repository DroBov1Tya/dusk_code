//! The standing goal, shown over the input.

use super::InputView;
use crate::app::{App, GoalButton, GoalHit};
use crate::ui::thread::{
    theme::{ACCENT, NEGATIVE, POSITIVE, RAISED, TEXT_SOFT, bold, fg},
    wrap,
};
use dusk_protocol::thread::GoalStatus;
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

pub(super) fn goal_rows(app: &App) -> u16 {
    u16::from(app.goal.is_some())
}

impl InputView<'_> {
    /// The messages waiting for the turn to end, each with its buttons; the newest are shown
    /// when there are more than fit.
    /// The standing goal in a line: its state, the round and the text, then its buttons.
    pub(super) fn draw_goal(&mut self, frame: &mut Frame, area: Rect) {
        self.app.goal_hits.clear();
        if area.is_empty() {
            return;
        }
        let Some(goal) = &self.app.goal else {
            return;
        };
        let color = match goal.status {
            _ if !goal.approved => ACCENT,
            GoalStatus::Paused => ACCENT,
            GoalStatus::Active | GoalStatus::Done => TEXT_SOFT,
        };
        let state = match goal.status {
            _ if !goal.approved && goal.criteria.is_empty() => "drafting criteria",
            _ if !goal.approved => "waiting for approval",
            _ => goal.verification.as_str(),
        };
        let name = if goal.approved {
            goal.status.name()
        } else {
            "draft"
        };
        let head = format!(
            "   \u{25CE} goal {name} [{state}]  {}/{}  ",
            goal.rounds, self.app.goal_rounds
        );
        let offered = crate::app::goal_buttons(goal);
        let buttons: usize = offered
            .iter()
            .map(|button| button.label().width() + 1)
            .sum();
        let room = (area.width as usize).saturating_sub(head.width() + buttons + 1);
        let text = wrap::clip(&goal.text.replace('\n', " "), room);
        let gap = room.saturating_sub(text.width()) + 1;
        let mut left = area.x + (head.width() + text.width() + gap) as u16;
        let mut spans = vec![
            Span::styled(head, bold(color)),
            Span::raw(text),
            Span::raw(" ".repeat(gap)),
        ];
        for &button in offered {
            let label = button.label();
            let tint = match button {
                GoalButton::Clear => NEGATIVE,
                GoalButton::Approve => POSITIVE,
                _ => color,
            };
            self.app.goal_hits.push(GoalHit {
                row: area.y,
                left,
                right: left + label.width() as u16,
                button,
            });
            spans.push(Span::styled(label, fg(tint).bg(RAISED)));
            spans.push(Span::raw(" "));
            left += label.width() as u16 + 1;
        }
        let line = Line::from(spans);
        frame.render_widget(Paragraph::new(line), area);
    }
}
