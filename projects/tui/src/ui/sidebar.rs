//! The left rail. Statistics always have a place; agent and MCP sections appear only when
//! their event sources have supplied something to show.

use dusk_core::{runtime::AUTO_COMPACT_PERCENT, state::ThreadTokens};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};

use super::{
    theme::{
        EMBER, NIGHT, SIDEBAR_BG, SIDEBAR_FG, SIDEBAR_MUTED, SUN, TEAL, VIOLET, bold, dusk, fg,
    },
    transcript::{one_line, spinner},
    wrap,
};
use crate::app::{App, compact_number, now_ms};

const STATS_HEIGHT: u16 = 9;
/// Pixels of a subagent figure: mirrored halves of a 5x6 grid, drawn in 5x3 cells.
const FIGURE_WIDTH: usize = 5;
const FIGURE_HALF: usize = 3;
const FIGURE_PIXELS: u32 = 18;

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
    let agents_active = !app.subagents.is_empty();
    let mcp_active = !app.active_mcp.is_empty();
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
                wrap::clip(&app.model, inner_width.saturating_sub(7)),
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

fn agents(frame: &mut Frame, area: Rect, app: &mut App) {
    let mut lines = Vec::new();
    let width = area.width.saturating_sub(10) as usize;
    let now = now_ms();
    let dots = spinner(app);
    for (index, agent) in app.subagents.iter().enumerate() {
        let seed = identity(&agent.thread_id);
        let color = icon_color(seed);
        let icon = icon_lines(seed);
        let name = agent_name(seed);
        let elapsed = agent
            .finished_at
            .unwrap_or(now)
            .saturating_sub(agent.started_at)
            .max(0) as u64;
        let age = format_duration(elapsed);
        // The random name stands out; the agent it works as and the task stay quiet.
        let state = if agent.finished_at.is_some() {
            "done"
        } else {
            dots
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", icon[0]), fg(color)),
            Span::styled(wrap::clip(&agent.agent, width), fg(SIDEBAR_MUTED)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", icon[1]), fg(color)),
            // Room left by the icon, the time and the dots.
            Span::styled(
                wrap::clip(&name, (area.width as usize).saturating_sub(19)),
                bold(color),
            ),
            Span::styled(format!(" {age} "), fg(SIDEBAR_MUTED)),
            Span::styled(state, fg(SUN)),
        ]));
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", icon[2]), fg(color)),
            Span::styled(wrap::clip(&one_line(&agent.task), width), fg(SIDEBAR_FG)),
        ]));
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

fn mcp(frame: &mut Frame, area: Rect, app: &mut App) {
    let lines: Vec<Line> = app
        .active_mcp
        .iter()
        .map(|name| Line::from(Span::styled(format!(" {}", name), fg(TEAL))))
        .collect();
    app.mcp_max_scroll = scroll_limit(lines.len(), area);
    app.mcp_scroll = app.mcp_scroll.min(app.mcp_max_scroll);
    let mut block = Block::bordered()
        .style(Style::new().bg(SIDEBAR_BG))
        .border_type(BorderType::Rounded)
        .border_style(fg(NIGHT))
        .title(Line::from(Span::styled(" MCP tools ", bold(TEAL))));
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

/// The core's agent ID is stable; its mixed bits choose a name, colour and geometry.
fn identity(id: &str) -> u64 {
    id.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn agent_name(seed: u64) -> String {
    const NAMES: [&str; 8] = [
        "Aster", "Beryl", "Cinder", "Dapple", "Ember", "Fable", "Glint", "Hollow",
    ];
    format!(
        "{}-{:02x}",
        NAMES[(seed as usize) % NAMES.len()],
        (seed >> 24) & 0xff
    )
}

fn icon_color(seed: u64) -> Color {
    Color::Rgb(
        110 + ((seed >> 8) & 0x7f) as u8,
        100 + ((seed >> 16) & 0x7f) as u8,
        125 + ((seed >> 32) & 0x7f) as u8,
    )
}

/// A figure that tells subagents apart: random pixels mirrored like a face, with half
/// blocks for two pixel rows per cell, always the same size.
fn icon_lines(seed: u64) -> [String; 3] {
    let mut bits = mix(seed) & ((1 << FIGURE_PIXELS) - 1);
    // A sparse figure reads as noise, so it is drawn as its negative instead.
    if bits.count_ones() < FIGURE_PIXELS / 3 {
        bits ^= (1 << FIGURE_PIXELS) - 1;
    }
    let pixel = |row: usize, column: usize| {
        let column = column.min(FIGURE_WIDTH - 1 - column);
        bits >> (row * FIGURE_HALF + column) & 1 == 1
    };
    std::array::from_fn(|row| {
        (0..FIGURE_WIDTH)
            .map(
                |column| match (pixel(row * 2, column), pixel(row * 2 + 1, column)) {
                    (true, true) => '\u{2588}',
                    (true, false) => '\u{2580}',
                    (false, true) => '\u{2584}',
                    (false, false) => ' ',
                },
            )
            .collect()
    })
}

/// Spreads the bits of the id hash, which the name and the color already use.
fn mix(seed: u64) -> u64 {
    let mut value = seed.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

fn format_duration(ms: u64) -> String {
    let seconds = ms / 1_000;
    let minutes = seconds / 60;
    if minutes >= 60 {
        format!("{}:{:02}:{:02}", minutes / 60, minutes % 60, seconds % 60)
    } else {
        format!("{minutes:02}:{:02}", seconds % 60)
    }
}
