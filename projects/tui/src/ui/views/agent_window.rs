//! One subagent's thread in a window over the screen: its numbers at the top, beside its
//! loader, and below them its conversation and actions as they happen.

use dusk_core::runtime::AUTO_COMPACT_PERCENT;
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};

use super::sidebar::format_duration;
use super::{
    loader::{Loader, agent_name, identity},
    panels::window,
    theme::{DOT, EMBER, GUTTER, NIGHT, SUN, TEAL, bold, dim, dusk, fg, thought},
    transcript::{one_line, thread_lines},
    wrap,
};
use crate::app::{App, Panel, TranscriptHit, compact_number};

/// The numbers above the conversation, and the rule under them.
const HEADER_ROWS: u16 = 4;

pub fn draw(frame: &mut Frame, screen: Rect, app: &mut App) {
    let Some(Panel::Agent(agent_window)) = &mut app.panel else {
        return;
    };
    let area = window(screen, 150);
    agent_window.area = area;
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
        .border_type(BorderType::Rounded)
        .border_style(fg(NIGHT))
        .title(Line::from(vec![
            Span::styled(format!(" {} ", agent_name(&thread_id)), bold(color)),
            Span::styled(format!("{DOT} {kind} "), dim()),
        ]))
        .title_bottom(
            Line::from(Span::styled(
                " wheel or PgUp/PgDn scroll  click opens  Ctrl+R thinking  esc close ",
                dim(),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    if inner.height <= HEADER_ROWS {
        return;
    }
    let width = inner.width as usize;
    let running = card.is_some_and(|card| card.finished_at.is_none());
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
            fg(NIGHT),
        ))])
        .collect();
    frame.render_widget(
        Paragraph::new(header),
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
            vec![Span::styled("no numbers yet", thought())],
            Vec::new(),
            Vec::new(),
        ];
    };
    let elapsed = format_duration(card.elapsed_ms());
    let state = match card.finished_at {
        None => Span::styled(format!("working {elapsed}"), bold(SUN)),
        Some(_) => Span::styled(format!("done in {elapsed}"), fg(TEAL)),
    };
    let parent = match &card.parent {
        Some(parent) if Some(parent) == app.thread_id.as_ref() => {
            app.title.clone().unwrap_or_else(|| "this chat".into())
        }
        Some(parent) => agent_name(parent),
        None => "this chat".into(),
    };
    let tokens = card.tokens;
    let fill = if card.limit == 0 {
        0.0
    } else {
        (tokens.context as f64 / card.limit as f64).clamp(0.0, 1.0)
    };
    let percent = fill * 100.0;
    let share_color = if percent >= AUTO_COMPACT_PERCENT as f64 {
        EMBER
    } else {
        dusk(fill)
    };
    let rate = card
        .tokens_per_second()
        .map_or("-- tok/s".into(), |rate| format!("~{rate:.1} tok/s"));
    [
        vec![
            state,
            Span::styled(format!("  {DOT}  asked by "), dim()),
            Span::raw(wrap::clip(&parent, width.saturating_sub(30))),
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
                dim(),
            ),
            Span::styled(rate, fg(TEAL)),
        ],
        vec![
            Span::styled(format!("task {GUTTER} "), dim()),
            Span::raw(wrap::clip(&one_line(&card.task), width.saturating_sub(8))),
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
    let visible: Vec<Line> = lines.into_iter().skip(top).take(height).collect();
    frame.render_widget(Paragraph::new(visible), area);
    if let Some(Panel::Agent(agent_window)) = &mut app.panel {
        agent_window.max_scroll = max_scroll;
        agent_window.scroll = scroll;
        agent_window.hits = hits;
    }
}
