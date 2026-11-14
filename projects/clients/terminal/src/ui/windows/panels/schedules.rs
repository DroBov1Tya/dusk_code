//! /schedules: the timetable, a line for each task with when it runs next.

use dusk_protocol::project::{Repeat, Schedule};
use ratatui::text::{Line, Span};

use super::frame::selectable;
use crate::ui::windows::theme::{NEGATIVE, dim, fg, thought};

pub fn schedules(
    tasks: &[Schedule],
    selected: usize,
    removing: bool,
) -> (Vec<Line<'static>>, usize) {
    if tasks.is_empty() {
        return (
            vec![Line::from(Span::styled(
                " No scheduled tasks; /schedule daily 09:00 <prompt> makes one",
                thought(),
            ))],
            0,
        );
    }
    let lines = tasks
        .iter()
        .enumerate()
        .map(|(index, task)| {
            let title = task.prompt.lines().next().unwrap_or_default().to_owned();
            let when = match task.repeat {
                Repeat::Once { at } => format!("once, {}", moment(at)),
                ref repeat => repeat.describe(),
            };
            let state = match (task.enabled, task.next_run_at) {
                (true, Some(at)) => format!("{when} \u{00B7} next {}", moment(at)),
                (true, None) => format!("{when} \u{00B7} done"),
                (false, _) => format!("{when} \u{00B7} paused"),
            };
            let right = if removing && index == selected {
                Span::styled("press d again to remove it".to_owned(), fg(NEGATIVE))
            } else {
                Span::styled(state, dim())
            };
            selectable(
                vec![Span::raw(format!(" {title}"))],
                right,
                index == selected,
            )
        })
        .collect();
    (lines, selected)
}

fn moment(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map_or("?".into(), |at| at.format("%a %-d %b %H:%M").to_string())
}
