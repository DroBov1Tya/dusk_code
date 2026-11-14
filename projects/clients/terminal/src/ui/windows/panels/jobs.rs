//! /jobs: background commands and their output.

use ratatui::text::{Line, Span};

use dusk_protocol::thread::JobInfo;

use super::frame::selectable;
use crate::app::JobsView;
use crate::ui::text::format::{format_duration, one_line, printable};
use crate::ui::windows::{
    theme::{ACCENT, DONE, DOT, FAILED, NEGATIVE, ON, POSITIVE, TEXT, TEXT_MUTED, bold, dim, fg},
    wrap,
};

/// Rows of the chosen job's output the window shows: the end, where a failure is.
pub const JOB_OUTPUT_ROWS: usize = 12;

/// The jobs, one row each, and under them what the chosen one printed.
pub fn jobs(view: &JobsView, width: usize) -> Vec<Line<'static>> {
    let now = dusk_protocol::system::now_ms();
    let mut lines: Vec<Line<'static>> = view
        .jobs
        .iter()
        .enumerate()
        .map(|(index, job)| {
            let (mark, color) = job_mark(job);
            let end = job.finished_at.unwrap_or(now);
            let time = format_duration(end.saturating_sub(job.started_at).max(0) as u64);
            selectable(
                vec![
                    Span::styled(format!(" {mark} "), fg(color)),
                    Span::styled(format!("{:<8}", short_id(&job.job_id)), dim()),
                    Span::styled(format!("{:<11}", wrap::clip(&job.status, 10)), fg(color)),
                    Span::styled(format!("{time:>6}  "), dim()),
                    Span::raw(wrap::clip(
                        &one_line(&job.command),
                        width.saturating_sub(32),
                    )),
                ],
                Span::raw(" "),
                index == view.selected,
            )
        })
        .collect();
    let Some(job) = view.jobs.get(view.selected) else {
        return lines;
    };
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        format!(" output of {}", short_id(&job.job_id)),
        bold(TEXT),
    )));
    // A finished job's exit code leads its output; it stays in view above the end.
    let (code, body) = match view.output.split_once('\n') {
        Some((first, rest)) if first.starts_with("exit code:") => (Some(first), rest),
        _ => (None, view.output.as_str()),
    };
    if let Some(code) = code {
        lines.push(Line::from(Span::styled(format!(" {code}"), dim())));
    }
    let printed: Vec<&str> = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if printed.is_empty() {
        lines.push(Line::from(Span::styled(" nothing printed yet", dim())));
    }
    let skip = printed.len().saturating_sub(JOB_OUTPUT_ROWS);
    if skip > 0 {
        lines.push(Line::from(Span::styled(
            format!(" ... {skip} earlier lines"),
            dim(),
        )));
    }
    lines.extend(printed.iter().skip(skip).map(|line| {
        Line::from(Span::raw(format!(
            " {}",
            wrap::clip(&printable(line), width.saturating_sub(2))
        )))
    }));
    lines
}

/// The last characters of a job's ID, which differ between jobs started together; the
/// first ones are a timestamp.
pub fn short_id(id: &str) -> &str {
    &id[id.len().saturating_sub(6)..]
}

pub fn job_mark(job: &JobInfo) -> (&'static str, ratatui::style::Color) {
    match job.status.as_str() {
        "completed" => (DONE, POSITIVE),
        "failed" | "timed_out" => (FAILED, NEGATIVE),
        "cancelled" | "interrupted" => (DOT, TEXT_MUTED),
        _ if job.running => (ON, ACCENT),
        _ => (DOT, TEXT_MUTED),
    }
}
