//! The left rail. Statistics always have a place; agent and MCP sections appear only when
//! their event sources have supplied something to show.

use dusk_core::{
    mcp::McpState,
    runtime::AUTO_COMPACT_PERCENT,
    state::{Task, ThreadTokens},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph},
};

use super::{
    chat::transcript::one_line,
    loader::{Loader, agent_name, identity},
    text::wrap,
    theme::{
        ACCENT, DEEP, DONE, DOT, FAILED, FOLDED, HAIRLINE, NEGATIVE, OFF, ON, POSITIVE, TEXT,
        TEXT_SOFT, UNFOLDED, bold, dusk, fg,
    },
};
use crate::app::{App, compact_number, now_ms};

/// Running jobs the rail lists before it sums up the rest.
const MAX_JOB_ROWS: usize = 4;
/// The statistics: fifteen lines and a blank one below.
const STATS_HEIGHT: u16 = 16;
/// The connectors folded: the heading and one line that sums them up.
const CONNECTORS_FOLDED: u16 = 2;
/// A compact bottom panel, kept the same height when connectors fold.
const CONNECTORS_HEIGHT: u16 = 5;
/// Columns of air on each side of the rail's text.
const PAD: u16 = 2;
const GAUGE: &str = "\u{2501}";
pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, overlay: bool) {
    if overlay {
        frame.render_widget(Clear, area);
    }
    frame.render_widget(Block::default().style(Style::new().bg(DEEP)), area);
    if area.width < 4 || area.height < 3 {
        return;
    }
    let stats_height = STATS_HEIGHT.min(area.height);
    statistics(
        frame,
        Rect {
            height: stats_height,
            ..area
        },
        app,
    );
    let mut y = area.y + stats_height;
    let remaining = area.bottom() - y;
    // Dock connectors before allocating the content above them. Their geometry
    // depends only on the rail, never on agents, tasks, or their folded state.
    let mcp_section = if !app.mcp.is_empty() && remaining >= CONNECTORS_FOLDED {
        let height = CONNECTORS_HEIGHT.min(remaining.div_ceil(2).max(CONNECTORS_FOLDED));
        Some(Rect {
            y: area.bottom() - height,
            height,
            ..area
        })
    } else {
        None
    };
    // Leave one blank row above the dock when the rail has room for it.
    let content_bottom = mcp_section
        .map(|section| section.y.saturating_sub(1).max(y))
        .unwrap_or(area.bottom());
    if app.tasks.iter().any(|task| !task.done) && y < content_bottom {
        let remaining = content_bottom - y;
        // The tasks take what they need, up to half of what is left.
        let room = remaining.div_ceil(2).max(3).min(remaining);
        y += tasks(
            frame,
            Rect {
                y,
                height: room,
                ..area
            },
            app,
        );
    }
    let running_jobs = app.jobs.iter().filter(|job| job.running).count();
    if running_jobs > 0 && y + 1 < content_bottom {
        let wanted = 1 + running_jobs.min(MAX_JOB_ROWS + 1) as u16;
        let height = wanted.min(content_bottom - y);
        y += background(frame, Rect { y, height, ..area }, app) + 1;
    }
    if !app.subagents.is_empty() && y < content_bottom {
        let section = Rect {
            y,
            height: content_bottom - y,
            ..area
        };
        app.agent_area = Some(section);
        agents(frame, section, app);
    }
    if let Some(section) = mcp_section {
        app.mcp_area = Some(section);
        mcp(frame, section, app);
    }
}

fn statistics(frame: &mut Frame, area: Rect, app: &App) {
    let width = area.width.saturating_sub(2 * PAD) as usize;
    let fill = if app.limit == 0 {
        0.0
    } else {
        (app.tokens.context as f64 / app.limit as f64).clamp(0.0, 1.0)
    };
    let percent = fill * 100.0;
    let near_compaction = percent >= AUTO_COMPACT_PERCENT as f64;
    let ThreadTokens {
        spent, model_calls, ..
    } = app.tokens;
    let elapsed = app
        .elapsed_ms()
        .map(format_duration)
        .unwrap_or_else(|| "--:--".into());
    let time_label = if app.turn_started_at.is_some() {
        "turn"
    } else {
        "last turn"
    };
    let calls = match model_calls {
        1 => " \u{00B7} 1 call".to_owned(),
        calls => format!(" \u{00B7} {calls} calls"),
    };
    let rate = app
        .approximate_tokens_per_second()
        .map(|value| format!("~{value:.1} tok/s"))
        .unwrap_or_else(|| "--".into());
    let context_color = if near_compaction {
        NEGATIVE
    } else {
        dusk(fill)
    };
    let mode = match app.effort {
        Some(effort) => format!(
            "{} \u{00B7} effort {}",
            app.mode.to_lowercase(),
            effort.name()
        ),
        None => app.mode.to_lowercase(),
    };
    let filled = ((width as f64) * fill).round() as usize;
    let lines = vec![
        Line::default(),
        Line::from(Span::styled(wrap::clip(&app.model, width), bold(TEXT))),
        Line::from(Span::styled(wrap::clip(&mode, width), fg(TEXT_SOFT))),
        Line::default(),
        row(
            vec![heading("Context")],
            Span::styled(format!("{percent:.0}%"), bold(context_color)),
            width,
        ),
        Line::from(vec![
            Span::styled(GAUGE.repeat(filled), fg(context_color)),
            Span::styled(GAUGE.repeat(width - filled), fg(HAIRLINE)),
        ]),
        Line::from(Span::styled(
            format!(
                "{} of {}{}",
                compact_number(app.tokens.context),
                compact_number(app.limit),
                if near_compaction {
                    "  compacting soon"
                } else {
                    ""
                }
            ),
            fg(TEXT_SOFT),
        )),
        Line::default(),
        Line::from(heading("Tokens")),
        token_row(
            "in / out",
            spent.prompt_tokens,
            spent.completion_tokens,
            width,
        ),
        token_row(
            "cache / think",
            spent.cached_prompt_tokens,
            spent.reasoning_tokens,
            width,
        ),
        Line::default(),
        Line::from(heading("Activity")),
        row(
            vec![Span::styled(format!("{time_label}{calls}"), fg(TEXT_SOFT))],
            Span::styled(elapsed, fg(ACCENT)),
            width,
        ),
        row(
            vec![Span::styled("output", fg(TEXT_SOFT))],
            Span::styled(rate, fg(TEXT_SOFT)),
            width,
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(TEXT).bg(DEEP))
            .block(Block::new().padding(Padding::horizontal(PAD))),
        area,
    );
}

/// A section's name, quiet and in capitals, above what it holds.
/// A section's name: sentence case, quieter than what it heads.
fn heading(text: &str) -> Span<'static> {
    Span::styled(text.to_owned(), fg(TEXT_SOFT))
}

/// Two counts after a label. On a narrow rail the decimals go first, so the numbers
/// never run into the label.
fn token_row(label: &'static str, first: u64, second: u64, width: usize) -> Line<'static> {
    let full = format!("{} / {}", compact_number(first), compact_number(second));
    let value = if label.len() + 1 + full.len() <= width {
        full
    } else {
        format!("{} / {}", whole_number(first), whole_number(second))
    };
    row(
        vec![Span::styled(label, fg(TEXT_SOFT))],
        Span::styled(value, fg(TEXT)),
        width,
    )
}

/// Like `compact_number`, rounded to a whole unit.
fn whole_number(value: u64) -> String {
    match value {
        0..1_000 => value.to_string(),
        1_000..1_000_000 => format!("{}k", (value + 500) / 1_000),
        _ => format!("{}M", (value + 500_000) / 1_000_000),
    }
}

/// `left` on the left and `right` against the right edge.
fn row(left: Vec<Span<'static>>, right: Span<'static>, width: usize) -> Line<'static> {
    let used: usize = left.iter().map(Span::width).sum::<usize>() + right.width();
    let mut spans = left;
    // At least one space, so a value never reads as part of its label.
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used).max(1))));
    spans.push(right);
    Line::from(spans)
}

/// A section below the statistics: its heading on the first row, then what it holds,
/// with the same air on both sides as the statistics.
fn section(title: Line<'static>) -> Block<'static> {
    let mut title = title;
    title.spans.insert(0, Span::raw(" ".repeat(PAD as usize)));
    Block::new()
        .style(Style::new().bg(DEEP))
        .title(title)
        .padding(Padding::horizontal(PAD))
}

/// The tasks still open, each in a small block of its own: what it is and how many steps
/// are done, and once unfolded its steps with how long each ran. Returns the rows used.
fn tasks(frame: &mut Frame, area: Rect, app: &mut App) -> u16 {
    let width = area.width.saturating_sub(2 * PAD) as usize;
    let now = now_ms();
    // Every open task's rows, a blank row between tasks, each row with its task.
    let mut rows: Vec<(Line<'static>, Option<String>)> = Vec::new();
    for task in app.tasks.iter().filter(|task| !task.done) {
        if !rows.is_empty() {
            rows.push((Line::default(), None));
        }
        let unfolded = app.unfolded_tasks.contains(&task.task_id);
        for line in task_lines(task, unfolded, width, now) {
            rows.push((line, Some(task.task_id.clone())));
        }
    }
    app.task_max_scroll = scroll_limit(rows.len(), area);
    app.task_scroll = app.task_scroll.min(app.task_max_scroll);
    app.task_area = Some(area);
    let mut block = section(Line::from(heading("Tasks")));
    let scrolls = app.task_max_scroll > 0 && area.height > 2;
    if scrolls {
        block = block.title_bottom(Line::from(Span::styled(" wheel to scroll ", fg(TEXT_SOFT))));
    }
    let inner = block.inner(area);
    let shown = rows
        .iter()
        .skip(app.task_scroll)
        .take(inner.height as usize);
    app.task_hits = shown
        .clone()
        .enumerate()
        .filter_map(|(row, (_, task_id))| {
            let task_id = task_id.clone()?;
            Some((
                Rect {
                    y: inner.y + row as u16,
                    height: 1,
                    ..area
                },
                task_id,
            ))
        })
        .collect();
    let lines: Vec<Line> = shown.map(|(line, _)| line.clone()).collect();
    let used = 1 + lines.len() as u16 + u16::from(scrolls);
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(TEXT).bg(DEEP))
            .block(block),
        Rect {
            height: used.min(area.height),
            ..area
        },
    );
    // A blank row before the next section.
    (used + 1).min(area.height)
}

/// The background jobs that run, with how long each has: the command and its time. The
/// rest are summed up, and `/jobs` opens them all. Returns the rows used.
fn background(frame: &mut Frame, area: Rect, app: &App) -> u16 {
    let width = area.width.saturating_sub(2 * PAD) as usize;
    let now = now_ms();
    let running: Vec<_> = app.jobs.iter().filter(|job| job.running).collect();
    let heading = Line::from(vec![
        heading("Background"),
        Span::styled("  /jobs", fg(TEXT_SOFT)),
    ]);
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

fn task_lines(task: &Task, unfolded: bool, width: usize, now: i64) -> Vec<Line<'static>> {
    let finished = task.items.iter().filter(|item| item.done).count();
    let count = format!(" {finished}/{}", task.items.len());
    let fold = if unfolded { UNFOLDED } else { FOLDED };
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{fold} "), fg(ACCENT)),
        Span::styled(
            fill(&task.description, width.saturating_sub(count.len() + 2)),
            bold(TEXT),
        ),
        Span::styled(count, fg(TEXT_SOFT)),
    ])];
    if !unfolded {
        return lines;
    }
    for item in &task.items {
        let (mark, color) = match (item.done, item.started_at) {
            (true, _) => (DONE, POSITIVE),
            (false, Some(_)) => (ON, ACCENT),
            (false, None) => (DOT, TEXT_SOFT),
        };
        // A running step counts up; a done one shows what it took.
        let time = item
            .started_at
            .map(|start| {
                let end = item.finished_at.unwrap_or(now);
                format!(
                    " {}",
                    format_duration(end.saturating_sub(start).max(0) as u64)
                )
            })
            .unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(format!("{mark} "), fg(color)),
            Span::styled(
                fill(&item.description, width.saturating_sub(time.len() + 2)),
                fg(if item.done { TEXT_SOFT } else { TEXT }),
            ),
            Span::styled(time, fg(if item.done { TEXT_SOFT } else { ACCENT })),
        ]));
    }
    lines
}

/// `text` cut to `width` and padded to it, so what follows lines up on the right.
fn fill(text: &str, width: usize) -> String {
    let clipped = wrap::clip(text, width);
    let pad = width.saturating_sub(clipped.chars().count());
    format!("{clipped}{}", " ".repeat(pad))
}

fn agents(frame: &mut Frame, area: Rect, app: &mut App) {
    let mut lines = Vec::new();
    let width = area.width.saturating_sub(8 + 2 * PAD) as usize;
    // When every agent does not fit in full, each takes two rows, so the last ones
    // still show what they work on.
    let full_rows = app.subagents.len() * 4 - 1;
    let compact = full_rows > area.height.saturating_sub(1) as usize;
    for (index, agent) in app.subagents.iter().enumerate() {
        let seed = identity(&agent.thread_id);
        let loader = Loader::new(seed);
        let color = loader.color();
        let running = agent.is_working();
        let [top, middle, bottom] = loader.rows(running.then_some(app.frame));
        let name = agent_name(&agent.thread_id);
        let age = format_duration(agent.elapsed_ms());
        let state = if agent.unavailable_at.is_some() {
            "unavailable"
        } else if running {
            ""
        } else {
            "done"
        };
        // Room left by the loader, the time and the status.
        let room = (area.width as usize).saturating_sub(13 + state.len() + 2 * PAD as usize);
        let named = vec![
            Span::raw(" "),
            Span::styled(wrap::clip(&name, room), bold(color)),
            Span::styled(format!(" {age} "), fg(TEXT_SOFT)),
            Span::styled(state, fg(ACCENT)),
        ];
        if compact {
            lines.push(Line::from([middle, named].concat()));
            let task = format!("{} \u{00B7} {}", agent.agent, one_line(&agent.task));
            lines.push(Line::from(
                [
                    bottom,
                    vec![
                        Span::raw(" "),
                        Span::styled(wrap::clip(&task, width), fg(TEXT)),
                    ],
                ]
                .concat(),
            ));
            continue;
        }
        // The random name stands out; the agent it works as and the task stay quiet.
        lines.push(Line::from(
            [
                top,
                vec![Span::raw(" ")],
                vec![Span::styled(wrap::clip(&agent.agent, width), fg(TEXT_SOFT))],
            ]
            .concat(),
        ));
        lines.push(Line::from([middle, named].concat()));
        lines.push(Line::from(
            [
                bottom,
                vec![
                    Span::raw(" "),
                    Span::styled(wrap::clip(&one_line(&agent.task), width), fg(TEXT)),
                ],
            ]
            .concat(),
        ));
        if index + 1 < app.subagents.len() {
            lines.push(Line::default());
        }
    }
    app.agent_max_scroll = scroll_limit(lines.len(), area);
    app.agent_scroll = app.agent_scroll.min(app.agent_max_scroll);
    let mut block = section(Line::from(heading("Subagents")));
    if app.agent_max_scroll > 0 && area.height > 2 {
        block = block.title_bottom(Line::from(Span::styled(" wheel to scroll ", fg(TEXT_SOFT))));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(TEXT).bg(DEEP))
            .block(block)
            .scroll((app.agent_scroll as u16, 0)),
        area,
    );
}

/// The MCP servers. Folded, one line sums them up; unfolded, each server shows how it
/// stands: its tools once ready, why it failed otherwise. A click folds and unfolds.
fn mcp(frame: &mut Frame, area: Rect, app: &mut App) {
    let width = area.width.saturating_sub(2 * PAD) as usize;
    let mut lines = Vec::new();
    let mark = if app.connectors_open {
        UNFOLDED
    } else {
        FOLDED
    };
    let title = Line::from(vec![
        heading("Connectors"),
        Span::styled(format!(" {mark}"), fg(TEXT_SOFT)),
    ]);
    if !app.connectors_open {
        let ready = app
            .mcp
            .iter()
            .filter(|server| matches!(server.state, McpState::Ready(_)))
            .count();
        let failed = app
            .mcp
            .iter()
            .filter(|server| matches!(server.state, McpState::Failed(_)))
            .count();
        let mut summary = vec![Span::styled(format!("{ON} {ready} ready"), fg(POSITIVE))];
        if failed > 0 {
            summary.push(Span::styled(
                format!("  {FAILED} {failed} failed"),
                fg(NEGATIVE),
            ));
        }
        let block = section(title);
        frame.render_widget(
            Paragraph::new(Line::from(summary))
                .style(Style::new().fg(TEXT).bg(DEEP))
                .block(block),
            area,
        );
        return;
    }
    for server in &app.mcp {
        let (mark, color, state) = match &server.state {
            McpState::Off => (OFF, TEXT_SOFT, "off".to_owned()),
            McpState::Starting => (OFF, ACCENT, "starting".to_owned()),
            McpState::Ready(tools) => {
                let plural = if tools.len() == 1 { "" } else { "s" };
                (ON, POSITIVE, format!("{} tool{plural}", tools.len()))
            }
            McpState::Failed(_) => (FAILED, NEGATIVE, "failed".to_owned()),
        };
        let room = width.saturating_sub(state.len() + 3);
        lines.push(Line::from(vec![
            Span::styled(format!("{mark} "), fg(color)),
            Span::styled(fill(&server.name, room), bold(TEXT)),
            Span::styled(format!(" {state}"), fg(TEXT_SOFT)),
        ]));
        if let McpState::Failed(error) = &server.state {
            lines.push(Line::from(Span::styled(
                format!(
                    "   {}",
                    wrap::clip(&one_line(error), width.saturating_sub(3))
                ),
                fg(TEXT_SOFT),
            )));
        }
    }
    app.mcp_max_scroll = scroll_limit(lines.len(), area);
    app.mcp_scroll = app.mcp_scroll.min(app.mcp_max_scroll);
    let mut block = section(title);
    if app.mcp_max_scroll > 0 && area.height > 2 {
        block = block.title_bottom(Line::from(Span::styled(" wheel to scroll ", fg(TEXT_SOFT))));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(TEXT).bg(DEEP))
            .block(block)
            .scroll((app.mcp_scroll as u16, 0)),
        area,
    );
}

/// Lines past the heading and, on overflow, the scroll hint below the body.
fn scroll_limit(line_count: usize, area: Rect) -> usize {
    let mut visible = area.height.saturating_sub(1) as usize;
    if line_count > visible && area.height > 2 {
        visible -= 1;
    }
    if visible == 0 {
        0
    } else {
        line_count.saturating_sub(visible).min(u16::MAX as usize)
    }
}

pub fn format_duration(ms: u64) -> String {
    let seconds = ms / 1_000;
    let minutes = seconds / 60;
    if minutes >= 60 {
        format!("{}:{:02}:{:02}", minutes / 60, minutes % 60, seconds % 60)
    } else {
        format!("{minutes:02}:{:02}", seconds % 60)
    }
}
