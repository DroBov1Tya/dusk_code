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
    widgets::{Block, BorderType, Clear, Paragraph},
};

use super::{
    loader::{Loader, agent_name, identity},
    theme::{
        DONE, DOT, EMBER, FAILED, FOLDED, NIGHT, OFF, ON, SIDEBAR_BG, SIDEBAR_FG, SIDEBAR_MUTED,
        SUN, TEAL, UNFOLDED, VIOLET, bold, dusk, fg,
    },
    transcript::one_line,
    wrap,
};
use crate::app::{App, compact_number, now_ms};

const STATS_HEIGHT: u16 = 9;
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
    let agents_active = !app.subagents.is_empty();
    let mcp_active = !app.mcp.is_empty();
    if agents_active && y < area.bottom() {
        let remaining = area.bottom() - y;
        let height = if mcp_active {
            remaining.div_ceil(2).max(3).min(remaining)
        } else {
            remaining
        };
        let section = Rect { y, height, ..area };
        app.agent_area = Some(section);
        agents(frame, section, app);
        y += height;
    }
    if mcp_active && y < area.bottom() {
        let section = Rect {
            y,
            height: area.bottom() - y,
            ..area
        };
        app.mcp_area = Some(section);
        mcp(frame, section, app);
    }
}

fn statistics(frame: &mut Frame, area: Rect, app: &App) {
    let inner_width = area.width.saturating_sub(3) as usize;
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
        "last"
    };
    let rate = app
        .approximate_tokens_per_second()
        .map(|value| format!("~{value:.1} tok/s"))
        .unwrap_or_else(|| "-- tok/s".into());
    let lines = vec![
        Line::from(vec![
            Span::styled(" model ", fg(SIDEBAR_MUTED)),
            Span::styled(
                wrap::clip(&model_label(app), inner_width.saturating_sub(7)),
                bold(VIOLET),
            ),
        ]),
        Line::from(vec![
            Span::styled(" context ", fg(SIDEBAR_MUTED)),
            Span::styled(
                format!("{percent:.0}%"),
                bold(if near_compaction { EMBER } else { dusk(fill) }),
            ),
            Span::styled(if near_compaction { "  compact" } else { "" }, fg(EMBER)),
        ]),
        Line::from(Span::styled(
            format!(
                " {} / {}",
                compact_number(app.tokens.context),
                compact_number(app.limit)
            ),
            fg(SIDEBAR_MUTED),
        )),
        Line::from(vec![
            Span::styled(" in ", fg(SIDEBAR_MUTED)),
            Span::styled(compact_number(spent.prompt_tokens), fg(TEAL)),
            Span::styled("   out ", fg(SIDEBAR_MUTED)),
            Span::styled(compact_number(spent.completion_tokens), fg(SUN)),
        ]),
        Line::from(Span::styled(
            format!(
                " cache {}  think {}",
                compact_number(spent.cached_prompt_tokens),
                compact_number(spent.reasoning_tokens)
            ),
            fg(SIDEBAR_MUTED),
        )),
        Line::from(vec![
            Span::styled(format!(" calls {model_calls}  "), fg(SIDEBAR_MUTED)),
            Span::styled(format!("{time_label} {elapsed}"), fg(SUN)),
        ]),
        Line::from(Span::styled(format!(" output {rate}"), fg(TEAL))),
    ];
    let block = Block::bordered()
        .style(Style::new().bg(SIDEBAR_BG))
        .border_type(BorderType::Rounded)
        .border_style(fg(NIGHT))
        .title(Line::from(Span::styled(" statistics ", bold(VIOLET))));
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().fg(SIDEBAR_FG).bg(SIDEBAR_BG))
            .block(block),
        area,
    );
}

/// The tasks still open, each in a small block of its own: what it is and how many steps
/// are done, and once unfolded its steps with how long each ran. Returns the rows used.
fn tasks(frame: &mut Frame, area: Rect, app: &mut App) -> u16 {
    let width = area.width.saturating_sub(2) as usize;
    let now = now_ms();
    let mut y = area.y;
    let mut hits = Vec::new();
    for task in app.tasks.iter().filter(|task| !task.done) {
        let unfolded = app.unfolded_tasks.contains(&task.task_id);
        let lines = task_lines(task, unfolded, width, now);
        let height = (lines.len() as u16 + 2).min(area.bottom() - y);
        if height < 3 {
            break;
        }
        let block_area = Rect { y, height, ..area };
        let block = Block::bordered()
            .style(Style::new().bg(SIDEBAR_BG))
            .border_type(BorderType::Rounded)
            .border_style(fg(if unfolded { SUN } else { NIGHT }));
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::new().fg(SIDEBAR_FG).bg(SIDEBAR_BG))
                .block(block),
            block_area,
        );
        hits.push((block_area, task.task_id.clone()));
        y += height;
    }
    app.task_hits = hits;
    y - area.y
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
    let width = area.width.saturating_sub(10) as usize;
    let now = now_ms();
    for (index, agent) in app.subagents.iter().enumerate() {
        let seed = identity(&agent.thread_id);
        let loader = Loader::new(seed);
        let color = loader.color();
        let running = agent.finished_at.is_none();
        let [top, middle, bottom] = loader.rows(running.then_some(app.frame));
        let name = agent_name(&agent.thread_id);
        let elapsed = agent
            .finished_at
            .unwrap_or(now)
            .saturating_sub(agent.started_at)
            .max(0) as u64;
        let age = format_duration(elapsed);
        let state = if running { "" } else { "done" };
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
                    // Room left by the loader, the time and "done".
                    Span::styled(
                        wrap::clip(&name, (area.width as usize).saturating_sub(19)),
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
    let mut block = Block::bordered()
        .style(Style::new().bg(SIDEBAR_BG))
        .border_type(BorderType::Rounded)
        .border_style(fg(NIGHT))
        .title(Line::from(Span::styled(" subagents ", bold(TEAL))));
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

/// Each MCP server with how it stands: its tools once ready, why it failed otherwise.
fn mcp(frame: &mut Frame, area: Rect, app: &mut App) {
    let width = area.width.saturating_sub(4) as usize;
    let mut lines = Vec::new();
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
            Span::styled(format!(" {mark} "), fg(color)),
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
    let mut block = Block::bordered()
        .style(Style::new().bg(SIDEBAR_BG))
        .border_type(BorderType::Rounded)
        .border_style(fg(NIGHT))
        .title(Line::from(Span::styled(" MCP ", bold(TEAL))));
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

fn scroll_limit(line_count: usize, area: Rect) -> usize {
    let visible = area.height.saturating_sub(2) as usize;
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

/// The model, and its effort when one is set.
fn model_label(app: &App) -> String {
    match app.effort {
        Some(effort) => format!("{} \u{00B7} {}", app.model, effort.name()),
        None => app.model.clone(),
    }
}
