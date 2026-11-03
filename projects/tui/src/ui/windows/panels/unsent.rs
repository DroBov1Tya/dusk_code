//! /resend: messages kept locally while no model answered.

use ratatui::text::{Line, Span};

use super::frame::selectable;
use crate::app::{App, paste};
use crate::ui::windows::{
    theme::{dim, thought},
    wrap,
};

pub fn unsent(app: &App, selected: usize, width: usize) -> (Vec<Line<'static>>, usize) {
    if app.local_unsent.is_empty() {
        return (
            vec![Line::from(Span::styled(
                " No local unsent messages",
                thought(),
            ))],
            0,
        );
    }
    let width = width.clamp(1, 70);
    let mut lines = vec![
        Line::from(Span::styled(
            "Saved locally. Only Enter on a selected message sends it.",
            dim(),
        )),
        Line::default(),
    ];
    let mut row = 0;
    for (index, message) in app.local_unsent.iter().enumerate() {
        if index == selected {
            row = lines.len();
        }
        lines.push(selectable(
            vec![Span::raw(format!(
                " {}. {}",
                index + 1,
                wrap::clip(
                    message.content.lines().next().unwrap_or(""),
                    width.saturating_sub(6)
                )
            ))],
            Span::styled("not sent", dim()),
            index == selected,
        ));
        if index == selected {
            let preview = paste::labelled(&message.content);
            let wrapped = wrap::words(vec![Span::raw(preview)], width.saturating_sub(3).max(1), 0);
            lines.extend(
                wrapped
                    .into_iter()
                    .take(6)
                    .map(|spans| Line::from([vec![Span::raw("   ")], spans].concat())),
            );
            lines.push(Line::default());
        }
    }
    (lines, row)
}
