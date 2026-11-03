//! One subagent's thread in a window over the screen: its numbers at the top, beside its
//! loader, and below them its conversation and actions as they happen.

use dusk_core::runtime::AUTO_COMPACT_PERCENT;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::{
    loader::{Loader, agent_name, identity},
    panels::window,
    theme::{
        ACCENT, DEEP, DOT, GUTTER, NEGATIVE, POSITIVE, RAISED, SELECTED, TEXT, TEXT_MUTED,
        TEXT_SOFT, bold, dusk, fg,
    },
    transcript::thread_lines,
    wrap,
};
use crate::app::{App, Panel, TEXT_COLUMN, TranscriptHit, compact_number};
use crate::ui::text::format::{format_duration, one_line};

const STOP: &str = " \u{25A0} stop ";

/// The numbers above the conversation, and the rule under them.
const HEADER_ROWS: u16 = 4;

pub fn draw(frame: &mut Frame, screen: Rect, app: &mut App) {
    let Some(Panel::Agent(agent_window)) = &mut app.panel else {
        return;
    };
    // Room is left around the window, so a click beside it closes it.
    let area = window(screen, 140);
    let area = Rect {
        x: area.x + 2,
        width: area.width.saturating_sub(4),
        y: area.y + 1,
        height: area.height.saturating_sub(2),
    };
    agent_window.area = area;
    agent_window.stop = None;
    let thread_id = agent_window.thread_id.clone();
    let seed = identity(&thread_id);
    let loader = Loader::new(seed);
    let color = loader.color();
    let card = app
        .subagents
        .iter()
        .find(|card| card.thread_id == thread_id);
    let kind = card.map_or("agent", |card| card.agent.as_str());
    let block = Block::bordered()
        .style(Style::new().fg(TEXT).bg(DEEP))
        .border_type(BorderType::Rounded)
        .border_style(fg(TEXT_MUTED))
        .title(Line::from(vec![
            Span::styled(format!(" {} ", agent_name(&thread_id)), bold(color)),
            Span::styled(format!("{DOT} {kind} "), fg(TEXT_SOFT)),
        ]))
        .title_bottom(
            Line::from(Span::styled(
                " drag copies  Ctrl+C copy  wheel/PgUp/PgDn scroll  click opens  Ctrl+R thinking  s stop  esc close ",
                fg(TEXT_SOFT),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    if inner.height <= HEADER_ROWS {
        agent_window.body = Rect::default();
        agent_window.selection = None;
        return;
    }
    let width = inner.width as usize;
    let running = card.is_some_and(|card| card.is_working());
    agent_window.stop = None;
    if running && area.width > STOP.len() as u16 + 4 {
        let left = area.right() - STOP.len() as u16 - 2;
        let button = Rect::new(left, area.y, STOP.len() as u16, 1);
        frame.render_widget(
            Paragraph::new(Span::styled(STOP, fg(NEGATIVE).bg(RAISED))),
            button,
        );
        agent_window.stop = Some((area.y, left, button.right()));
    }
    let rows = loader.rows(running.then_some(app.frame));
    let stats = numbers(app, &thread_id, width.saturating_sub(7));
    let header: Vec<Line> = rows
        .into_iter()
        .zip(stats)
        .map(|(icon, numbers)| {
            let mut spans = vec![Span::raw(" ")];
            spans.extend(icon);
            spans.push(Span::raw(" "));
            spans.extend(numbers);
            Line::from(spans)
        })
        .chain([Line::from(Span::styled(
            "\u{2500}".repeat(width),
            fg(TEXT_MUTED),
        ))])
        .collect();
    frame.render_widget(
        Paragraph::new(header).style(Style::new().fg(TEXT).bg(RAISED)),
        Rect {
            height: HEADER_ROWS,
            ..inner
        },
    );
    let body = Rect {
        y: inner.y + HEADER_ROWS,
        height: inner.height - HEADER_ROWS,
        ..inner
    };
    conversation(frame, body, app);
}

/// Three rows: how it stands and who asked it, its tokens, its task.
fn numbers(app: &App, thread_id: &str, width: usize) -> [Vec<Span<'static>>; 3] {
    let Some(card) = app
        .subagents
        .iter()
        .find(|card| card.thread_id == thread_id)
    else {
        return [
            vec![Span::styled("no numbers yet", fg(TEXT_SOFT))],
            Vec::new(),
            Vec::new(),
        ];
    };
    let elapsed = format_duration(card.elapsed_ms());
    let state = match (&card.phase, card.unavailable_at, card.is_working()) {
        (_, Some(_), _) => Span::styled(format!("unavailable after {elapsed}"), fg(NEGATIVE)),
        (Some((phase, note)), _, working) => {
            let about = if note.is_empty() {
                String::new()
            } else {
                format!(": {note}")
            };
            let style = if working { bold(ACCENT) } else { fg(TEXT_SOFT) };
            Span::styled(format!("{} {elapsed}{about}", phase.name()), style)
        }
        (None, _, true) => Span::styled(format!("working {elapsed}"), bold(ACCENT)),
        (None, _, false) => Span::styled(format!("done in {elapsed}"), fg(POSITIVE)),
    };
    let parent = match &card.parent {
        Some(parent) if Some(parent) == app.thread_id.as_ref() => {
            app.title.clone().unwrap_or_else(|| "this chat".into())
        }
        Some(parent) => agent_name(parent),
        None => "this chat".into(),
    };
    let effort = card.effort.map_or("default", |effort| effort.name());
    let tokens = card.tokens;
    let fill = if card.limit == 0 {
        0.0
    } else {
        (tokens.context as f64 / card.limit as f64).clamp(0.0, 1.0)
    };
    let percent = fill * 100.0;
    let share_color = if percent >= AUTO_COMPACT_PERCENT as f64 {
        NEGATIVE
    } else {
        dusk(fill)
    };
    let rate = card
        .tokens_per_second()
        .map_or("-- tok/s".into(), |rate| format!("~{rate:.1} tok/s"));
    [
        vec![
            state,
            Span::styled(format!("  {DOT}  effort "), fg(TEXT_SOFT)),
            Span::styled(effort, bold(TEXT)),
            Span::styled(format!("  {DOT}  asked by "), fg(TEXT_SOFT)),
            Span::raw(wrap::clip(&parent, width.saturating_sub(48))),
        ],
        vec![
            Span::styled(format!("context {percent:.0}%"), bold(share_color)),
            Span::styled(
                format!(
                    " {} / {}  {DOT}  in {}  out {}  cache {}  think {}  {DOT}  calls {}  {DOT}  ",
                    compact_number(tokens.context),
                    compact_number(card.limit),
                    compact_number(tokens.spent.prompt_tokens),
                    compact_number(tokens.spent.completion_tokens),
                    compact_number(tokens.spent.cached_prompt_tokens),
                    compact_number(tokens.spent.reasoning_tokens),
                    tokens.model_calls,
                ),
                fg(TEXT_SOFT),
            ),
            Span::styled(rate, fg(TEXT_SOFT)),
        ],
        vec![
            Span::styled(format!("task {GUTTER} "), fg(TEXT_SOFT)),
            Span::styled(
                wrap::clip(&one_line(&card.task), width.saturating_sub(8)),
                bold(TEXT),
            ),
        ],
    ]
}

/// The agent's thread, newest at the bottom unless scrolled up.
fn conversation(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(Panel::Agent(agent_window)) = &app.panel else {
        return;
    };
    let (lines, starts) = thread_lines(&agent_window.transcript, area.width as usize, None, None);
    let height = area.height as usize;
    let total = lines.len().saturating_sub(1);
    let max_scroll = total.saturating_sub(height);
    let scroll = agent_window.scroll.min(max_scroll);
    let top = total.saturating_sub(height + scroll);
    let hits: Vec<TranscriptHit> = starts
        .iter()
        .enumerate()
        .filter(|&(index, &start)| {
            agent_window.transcript.is_foldable(index) && start >= top && start < top + height
        })
        .map(|(index, &start)| TranscriptHit {
            row: area.y + (start - top) as u16,
            left: area.x,
            right: area.right(),
            index,
        })
        .collect();
    let text: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect();
    let visible: Vec<Line> = lines.into_iter().skip(top).take(height).collect();
    frame.render_widget(
        Paragraph::new(visible).style(Style::new().fg(TEXT).bg(DEEP)),
        area,
    );
    if let Some(Panel::Agent(agent_window)) = &mut app.panel {
        if agent_window.body.x != area.x || agent_window.body.width != area.width {
            agent_window.selection = None;
        }
        agent_window.body = area;
        agent_window.drawn_top = top;
        agent_window.drawn_text = text;
        agent_window.max_scroll = max_scroll;
        agent_window.scroll = scroll;
        agent_window.hits = hits;
        if let Some(selection) = agent_window.selection {
            let (start, end) = selection.ordered();
            for row in 0..area.height {
                let line = top + row as usize;
                if line < start.0 || line > end.0 {
                    continue;
                }
                let width = agent_window
                    .drawn_text
                    .get(line)
                    .map_or(0, |text| text.width().min(area.width as usize) as u16);
                let from = if line == start.0 { start.1 } else { area.x }
                    .max(area.x.saturating_add(TEXT_COLUMN));
                let to = if line == end.0 {
                    end.1.saturating_add(1)
                } else {
                    area.right()
                }
                .min(area.x + width)
                .min(area.right());
                for x in from..to {
                    frame.buffer_mut()[(x, area.y + row)].set_bg(SELECTED);
                }
            }
        }
    }
}
