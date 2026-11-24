//! The standing goal, shown over the input.

use super::InputView;
use crate::app::{App, GoalButton, GoalHit};
use crate::ui::thread::{
    theme::{ACCENT, NEGATIVE, RAISED, TEXT_SOFT, bold, fg},
    wrap,
};
use dusk_protocol::{system::now_ms, thread::GoalStatus};
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
    /// The standing goal in a line: whether it goes on, for how long, the turns judged and
    /// the condition, then its button.
    pub(super) fn draw_goal(&mut self, frame: &mut Frame, area: Rect) {
        self.app.goal_hits.clear();
        if area.is_empty() {
            return;
        }
        let Some(goal) = &self.app.goal else {
            return;
        };
        let color = match goal.status {
            GoalStatus::Active => TEXT_SOFT,
            GoalStatus::Paused => ACCENT,
        };
        let mut head = format!("   {}", goal.indicator(now_ms()));
        if goal.turns > 0 {
            head.push_str(&format!(" \u{00B7} turn {}", goal.turns));
        }
        head.push_str("  ");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_app;
    use dusk_protocol::thread::Goal;
    use ratatui::{Terminal, backend::TestBackend};

    /// The goal's row drawn `width` columns wide, as text.
    fn row(app: &mut App, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).expect("test terminal");
        terminal
            .draw(|frame| InputView { app }.draw_goal(frame, frame.area()))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        (0..width).map(|x| buffer[(x, 0)].symbol()).collect()
    }

    #[test]
    fn the_row_says_the_goal_is_active_for_how_long_and_clears_it() {
        let mut app = test_app();
        app.goal = Some(Goal {
            thread_id: "t".into(),
            text: "all tests pass".into(),
            status: GoalStatus::Active,
            started_at: dusk_protocol::system::now_ms() - 125_000,
            turns: 2,
            tokens: 9_000,
            reason: Some("run the tests".into()),
            note: None,
        });
        assert_eq!(goal_rows(&app), 1);
        let drawn = row(&mut app, 80);
        println!("{}", drawn.trim_end());
        assert!(
            drawn.contains("\u{25CE} goal active \u{00B7} 2m 5s \u{00B7} turn 2  all tests pass"),
            "{drawn}"
        );
        assert!(drawn.trim_end().ends_with('\u{00D7}'), "{drawn}");
        assert_eq!(app.goal_hits.len(), 1);
    }
}
