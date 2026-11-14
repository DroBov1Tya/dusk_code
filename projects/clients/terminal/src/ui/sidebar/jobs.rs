//! The background jobs that run, each with its command and how long it has run.

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use super::{
    Sidebar,
    section::{PAD, fill, ruled, section},
};
use crate::app::now_ms;
use crate::ui::{
    text::format::{format_duration, one_line},
    theme::{ACCENT, DEEP, ON, TEXT, TEXT_MUTED, TEXT_SOFT, fg},
};

/// Running jobs the rail lists before it sums up the rest.
pub const MAX_JOB_ROWS: usize = 4;

impl Sidebar<'_> {
    /// The background jobs that run, with how long each has: the command and its time. The
    /// rest are summed up, and `/jobs` opens them all. Returns the rows used.
    pub fn background(&self, frame: &mut Frame, area: Rect) -> u16 {
        let width = area.width.saturating_sub(2 * PAD) as usize;
        let now = now_ms();
        let running: Vec<_> = self.app.jobs.iter().filter(|job| job.running).collect();
        let heading = ruled(
            "Background",
            Some(Span::styled("/jobs", fg(TEXT_MUTED))),
            width,
        );
        frame.render_widget(section(heading), Rect { height: 1, ..area });
        let mut lines: Vec<Line<'static>> = running
            .iter()
            .take(MAX_JOB_ROWS)
            .map(|job| {
                let time = format_duration(now.saturating_sub(job.started_at).max(0) as u64);
                Line::from(vec![
                    Span::styled(format!("{ON} "), fg(ACCENT)),
                    Span::styled(
                        fill(
                            &one_line(&job.command),
                            width.saturating_sub(time.len() + 3),
                        ),
                        fg(TEXT),
                    ),
                    Span::styled(format!(" {time}"), fg(ACCENT)),
                ])
            })
            .collect();
        if running.len() > MAX_JOB_ROWS {
            lines.push(Line::from(Span::styled(
                format!("+{} more", running.len() - MAX_JOB_ROWS),
                fg(TEXT_SOFT),
            )));
        }
        let rows = (lines.len() as u16).min(area.height.saturating_sub(1));
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::new().fg(TEXT).bg(DEEP))
                .block(Block::new().padding(Padding::horizontal(PAD))),
            Rect {
                y: area.y + 1,
                height: rows,
                ..area
            },
        );
        1 + rows
    }
}
