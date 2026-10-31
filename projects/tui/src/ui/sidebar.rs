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
        DONE, DOT, EMBER, FAILED, FOLDED, NIGHT, OFF, ON, SIDEBAR_BG, SIDEBAR_EDGE, SIDEBAR_FG,
        SIDEBAR_MUTED, SUN, TEAL, UNFOLDED, VIOLET, bold, dusk, fg,
    },
};
use crate::app::{App, compact_number, now_ms};

/// Running jobs the rail lists before it sums up the rest.
const MAX_JOB_ROWS: usize = 4;
/// The statistics: fifteen lines and a blank one below.
const STATS_HEIGHT: u16 = 16;
/// The connectors folded: the heading and one line that sums them up.
const CONNECTORS_FOLDED: u16 = 2;
/// Columns of air on each side of the rail's text.
const PAD: u16 = 2;
const GAUGE: &str = "\u{2501}";
pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, overlay: bool) {
    if overlay {
        frame.render_widget(Clear, area);
    }
    frame.render_widget(Block::default().style(Style::new().bg(SIDEBAR_BG)), area);
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
    if app.tasks.iter().any(|task| !task.done) && y < area.bottom() {
        let remaining = area.bottom() - y;
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
    if running_jobs > 0 && y + 1 < area.bottom() {
        let wanted = 1 + running_jobs.min(MAX_JOB_ROWS + 1) as u16;
        let height = wanted.min(area.bottom() - y);
        y += background(frame, Rect { y, height, ..area }, app) + 1;
    }
    let agents_active = !app.subagents.is_empty();
    let mcp_active = !app.mcp.is_empty();
    if agents_active && y < area.bottom() {
        let remaining = area.bottom() - y;
        let height = match (mcp_active, app.connectors_open) {
            (false, _) => remaining,
            (true, false) => remaining
                .saturating_sub(CONNECTORS_FOLDED + 1)
                .max(3)
                .min(remaining),
            (true, true) => remaining.div_ceil(2).max(3).min(remaining),
        };
        let section = Rect { y, height, ..area };
        app.agent_area = Some(section);
        agents(frame, section, app);
        // A blank row before the next section.
        y += height + 1;
    }
    if mcp_active && y + 1 < area.bottom() {
        let height = match app.connectors_open {
            true => area.bottom() - y,
            false => CONNECTORS_FOLDED.min(area.bottom() - y),
        };
        let section = Rect { y, height, ..area };
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
    let context_color = if near_compaction { EMBER } else { dusk(fill) };
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
        Line::from(Span::styled(wrap::clip(&app.model, width), bold(VIOLET))),
        Line::from(Span::styled(wrap::clip(&mode, width), fg(SIDEBAR_MUTED))),
        Line::default(),
        row(
            vec![heading("CONTEXT")],
            Span::styled(format!("{percent:.0}%"), bold(context_color)),
            width,
        ),
        Line::from(vec![
            Span::styled(GAUGE.repeat(filled), fg(context_color)),
            Span::styled(GAUGE.repeat(width - filled), fg(NIGHT)),
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
            fg(SIDEBAR_MUTED),
        )),
        Line::default(),
        Line::from(heading("TOKENS")),
        row(
            vec![Span::styled("in / out", fg(SIDEBAR_MUTED))],
            Span::styled(
                format!(
                    "{} / {}",
                    compact_number(spent.prompt_tokens),
                    compact_number(spent.completion_tokens)
                ),
                fg(SIDEBAR_FG),
            ),
            width,
        ),
        row(
            vec![Span::styled("cache / think", fg(SIDEBAR_MUTED))],
            Span::styled(
                format!(
                    "{} / {}",
                    compact_number(spent.cached_prompt_tokens),
                    compact_number(spent.reasoning_tokens)
                ),
                fg(SIDEBAR_FG),
            ),
            width,
        ),
        Line::default(),
        Line::from(heading("ACTIVITY")),
        row(
            vec![Span::styled(
                format!("{time_label}{calls}"),
                fg(SIDEBAR_MUTED),
            )],
            Span::styled(elapsed, fg(SUN)),
            width,
        ),
        row(
            vec![Span::styled("output", fg(SIDEBAR_MUTED))],
            Span::styled(rate, fg(TEAL)),
            width,
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(SIDEBAR_FG).bg(SIDEBAR_BG))
            .block(Block::new().padding(Padding::horizontal(PAD))),
        area,
    );
}

/// A section's name, quiet and in capitals, above what it holds.
fn heading(text: &str) -> Span<'static> {
    Span::styled(text.to_owned(), bold(SIDEBAR_EDGE))
}

/// `left` on the left and `right` against the right edge.
fn row(left: Vec<Span<'static>>, right: Span<'static>, width: usize) -> Line<'static> {
    let used: usize = left.iter().map(Span::width).sum::<usize>() + right.width();
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
    spans.push(right);
    Line::from(spans)
}

/// A section below the statistics: its heading on the first row, then what it holds,
/// with the same air on both sides as the statistics.
fn section(title: Line<'static>) -> Block<'static> {
    let mut title = title;
    title.spans.insert(0, Span::raw(" ".repeat(PAD as usize)));
    Block::new()
        .style(Style::new().bg(SIDEBAR_BG))
        .title(title)
        .padding(Padding::horizontal(PAD))
}

/// The tasks still open, each in a small block of its own: what it is and how many steps
/// are done, and once unfolded its steps with how long each ran. Returns the rows used.
fn tasks(frame: &mut Frame, area: Rect, app: &mut App) -> u16 {
    let width = area.width.saturating_sub(2 * PAD) as usize;
    let now = now_ms();
    let heading_area = Rect { height: 1, ..area };
    frame.render_widget(section(Line::from(heading("TASKS"))), heading_area);
    let mut y = area.y + 1;
    let mut hits = Vec::new();
    for task in app.tasks.iter().filter(|task| !task.done) {
        let unfolded = app.unfolded_tasks.contains(&task.task_id);
        let lines = task_lines(task, unfolded, width, now);
        let height = (lines.len() as u16).min(area.bottom() - y);
        if height == 0 {
            break;
        }
        let block_area = Rect { y, height, ..area };
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::new().fg(SIDEBAR_FG).bg(SIDEBAR_BG))
                .block(Block::new().padding(Padding::horizontal(PAD))),
            block_area,
        );
        hits.push((block_area, task.task_id.clone()));
        // A blank row between tasks.
        y = (y + height + 1).min(area.bottom());
    }
    app.task_hits = hits;
    y - area.y
}

/// The background jobs that run, with how long each has: the command and its time. The
/// rest are summed up, and `/jobs` opens them all. Returns the rows used.
fn background(frame: &mut Frame, area: Rect, app: &App) -> u16 {
    let width = area.width.saturating_sub(2 * PAD) as usize;
    let now = now_ms();
    let running: Vec<_> = app.jobs.iter().filter(|job| job.running).collect();
    let heading = Line::from(vec![
        heading("BACKGROUND"),
        Span::styled("  /jobs", fg(SIDEBAR_MUTED)),
    ]);
    frame.render_widget(section(heading), Rect { height: 1, ..area });
    let mut lines: Vec<Line<'static>> = running
        .iter()
        .take(MAX_JOB_ROWS)
        .map(|job| {
            let time = format_duration(now.saturating_sub(job.started_at).max(0) as u64);
            Line::from(vec![
                Span::styled(format!("{ON} "), fg(SUN)),
                Span::styled(
                    fill(
                        &one_line(&job.command),
                        width.saturating_sub(time.len() + 3),
                    ),
                    fg(SIDEBAR_FG),
                ),
                Span::styled(format!(" {time}"), fg(SUN)),
            ])
        })
        .collect();
    if running.len() > MAX_JOB_ROWS {
        lines.push(Line::from(Span::styled(
            format!("+{} more", running.len() - MAX_JOB_ROWS),
            fg(SIDEBAR_MUTED),
        )));
    }
    let rows = (lines.len() as u16).min(area.height.saturating_sub(1));
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(SIDEBAR_FG).bg(SIDEBAR_BG))
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
        Span::styled(format!("{fold} "), fg(SUN)),
        Span::styled(
            fill(&task.description, width.saturating_sub(count.len() + 2)),
            bold(SIDEBAR_FG),
        ),
        Span::styled(count, fg(SIDEBAR_MUTED)),
    ])];
    if !unfolded {
        return lines;
    }
    for item in &task.items {
        let (mark, color) = match (item.done, item.started_at) {
            (true, _) => (DONE, TEAL),
            (false, Some(_)) => (ON, SUN),
            (false, None) => (DOT, SIDEBAR_MUTED),
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
                fg(if item.done { SIDEBAR_MUTED } else { SIDEBAR_FG }),
            ),
            Span::styled(time, fg(if item.done { SIDEBAR_MUTED } else { SUN })),
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
        // The random name stands out; the agent it works as and the task stay quiet.
        lines.push(Line::from(
            [
                top,
                vec![Span::raw(" ")],
                vec![Span::styled(
                    wrap::clip(&agent.agent, width),
                    fg(SIDEBAR_MUTED),
                )],
            ]
            .concat(),
        ));
        lines.push(Line::from(
            [
                middle,
                vec![
                    Span::raw(" "),
                    // Room left by the loader, the time and the status.
                    Span::styled(
                        wrap::clip(
                            &name,
                            (area.width as usize)
                                .saturating_sub(13 + state.len() + 2 * PAD as usize),
                        ),
                        bold(color),
                    ),
                    Span::styled(format!(" {age} "), fg(SIDEBAR_MUTED)),
                    Span::styled(state, fg(SUN)),
                ],
            ]
            .concat(),
        ));
        lines.push(Line::from(
            [
                bottom,
                vec![
                    Span::raw(" "),
                    Span::styled(wrap::clip(&one_line(&agent.task), width), fg(SIDEBAR_FG)),
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
    let mut block = section(Line::from(heading("SUBAGENTS")));
    if app.agent_max_scroll > 0 {
        block = block.title_bottom(Line::from(Span::styled(
            " wheel to scroll ",
            fg(SIDEBAR_MUTED),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(SIDEBAR_FG).bg(SIDEBAR_BG))
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
        heading("CONNECTORS"),
        Span::styled(format!(" {mark}"), fg(SIDEBAR_MUTED)),
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
        let mut summary = vec![Span::styled(format!("{ON} {ready} ready"), fg(TEAL))];
        if failed > 0 {
            summary.push(Span::styled(
                format!("  {FAILED} {failed} failed"),
                fg(EMBER),
            ));
        }
        let block = section(title);
        frame.render_widget(
            Paragraph::new(Line::from(summary))
                .style(Style::new().fg(SIDEBAR_FG).bg(SIDEBAR_BG))
                .block(block),
            area,
        );
        return;
    }
    for server in &app.mcp {
        let (mark, color, state) = match &server.state {
            McpState::Off => (OFF, SIDEBAR_MUTED, "off".to_owned()),
            McpState::Starting => (OFF, SUN, "starting".to_owned()),
            McpState::Ready(tools) => {
                let plural = if tools.len() == 1 { "" } else { "s" };
                (ON, TEAL, format!("{} tool{plural}", tools.len()))
            }
            McpState::Failed(_) => (FAILED, EMBER, "failed".to_owned()),
        };
        let room = width.saturating_sub(state.len() + 3);
        lines.push(Line::from(vec![
            Span::styled(format!("{mark} "), fg(color)),
            Span::styled(fill(&server.name, room), bold(SIDEBAR_FG)),
            Span::styled(format!(" {state}"), fg(SIDEBAR_MUTED)),
        ]));
        if let McpState::Failed(error) = &server.state {
            lines.push(Line::from(Span::styled(
                format!(
                    "   {}",
                    wrap::clip(&one_line(error), width.saturating_sub(3))
                ),
                fg(SIDEBAR_MUTED),
            )));
        }
    }
    app.mcp_max_scroll = scroll_limit(lines.len(), area);
    app.mcp_scroll = app.mcp_scroll.min(app.mcp_max_scroll);
    let mut block = section(title);
    if app.mcp_max_scroll > 0 {
        block = block.title_bottom(Line::from(Span::styled(
            " wheel to scroll ",
            fg(SIDEBAR_MUTED),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(SIDEBAR_FG).bg(SIDEBAR_BG))
            .block(block)
            .scroll((app.mcp_scroll as u16, 0)),
        area,
    );
}

/// Lines past what a section shows below its heading.
fn scroll_limit(line_count: usize, area: Rect) -> usize {
    let visible = area.height.saturating_sub(1) as usize;
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
