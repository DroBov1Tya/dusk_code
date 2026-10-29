//! Panels over the transcript: threads, tool and skill switches, and the keys.

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::{
    theme::{GLOW, NIGHT, OFF, ON, SUN, SUN_GLYPH, VIOLET, bold, dim, fg, thought},
    wrap,
};
use crate::app::{App, Panel, SettingKind, SettingList, ThreadList};

const KEYS: [(&str, &str); 18] = [
    ("Enter", "send; it waits if the agent is busy"),
    ("Alt+Enter", "send now, interrupting the agent"),
    ("Esc", "stop the agent"),
    ("Ctrl+J", "new line"),
    ("Up / Down", "earlier messages you sent"),
    ("Tab", "walk thinking, tools and summaries"),
    ("Enter / Space", "fold or unfold the selected one"),
    ("Left click", "fold or unfold thinking, tools and summaries"),
    ("Ctrl+R", "fold or unfold all thinking"),
    ("PgUp / PgDn", "scroll the transcript; wheel works too"),
    ("Shift+PgUp/Dn", "scroll subagent cards"),
    ("Alt+PgUp/Dn", "scroll MCP tools"),
    ("Ctrl+T", "threads"),
    ("Ctrl+N", "new thread"),
    ("Ctrl+O", "tools and skills"),
    ("F2", "show or hide the statistics sidebar"),
    ("/compact", "summarize the context to free it"),
    ("Ctrl+C", "quit"),
];

pub fn draw(frame: &mut Frame, screen: Rect, app: &App) {
    let Some(panel) = &app.panel else {
        return;
    };
    let (title, hints, lines, selected) = match panel {
        Panel::Threads(list) => {
            let current = app.thread_id.as_deref();
            (
                "threads",
                "enter open  n new  esc close",
                threads(list, current),
                Some(list.selected),
            )
        }
        Panel::Settings(list) => {
            let (lines, row) = settings(list);
            (
                "tools and skills",
                "space switch  r reload config  esc close",
                lines,
                Some(row),
            )
        }
        Panel::Help => ("keys", "any key closes", help(), None),
    };
    let width = screen.width.saturating_sub(4).min(76);
    let height = (lines.len() as u16 + 2)
        .min(screen.height.saturating_sub(4))
        .max(3);
    let area = Rect {
        x: screen.x + (screen.width - width) / 2,
        y: screen.y + (screen.height - height) / 3,
        width,
        height,
    };
    let inner = height.saturating_sub(2) as usize;
    let skip = selected.map_or(0, |row| (row + 1).saturating_sub(inner));
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(NIGHT))
        .title(Line::from(Span::styled(format!(" {title} "), bold(VIOLET))))
        .title_bottom(Line::from(Span::styled(format!(" {hints} "), dim())).right_aligned());
    let lines = fit(lines, width.saturating_sub(2) as usize);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(block).scroll((skip as u16, 0)),
        area,
    );
}

fn threads(list: &ThreadList, current: Option<&str>) -> Vec<Line<'static>> {
    if list.threads.is_empty() {
        return vec![Line::from(Span::styled(" no threads yet", thought()))];
    }
    let now = now_ms();
    list.threads
        .iter()
        .enumerate()
        .map(|(row, thread)| {
            let mark = if current == Some(thread.id.as_str()) {
                Span::styled(format!(" {SUN_GLYPH} "), fg(SUN))
            } else {
                Span::raw("   ")
            };
            let title = match &thread.title {
                Some(title) => Span::raw(title.clone()),
                None => Span::styled("no messages", thought()),
            };
            let age = Span::styled(age(now - thread.active_at), dim());
            selectable(vec![mark, title], age, row == list.selected)
        })
        .collect()
}

/// The lines, and the line of the selected item.
fn settings(list: &SettingList) -> (Vec<Line<'static>>, usize) {
    let mut lines = Vec::new();
    let mut selected_line = 0;
    let mut section = None;
    for (index, item) in list.items.iter().enumerate() {
        if section != Some(item.kind) {
            section = Some(item.kind);
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            let name = match item.kind {
                SettingKind::Tool => "tools",
                SettingKind::Skill => "skills",
            };
            lines.push(Line::from(Span::styled(
                format!(" {name}"),
                dim().add_modifier(Modifier::BOLD),
            )));
        }
        let switch = if item.on {
            Span::styled(format!("  {ON} "), fg(SUN))
        } else {
            Span::styled(format!("  {OFF} "), fg(NIGHT))
        };
        let name_style = if item.on { Style::new() } else { dim() };
        let about = Span::styled(item.about.clone(), dim());
        if index == list.selected {
            selected_line = lines.len();
        }
        lines.push(selectable(
            vec![switch, Span::styled(item.name.clone(), name_style)],
            about,
            index == list.selected,
        ));
    }
    if list.items.is_empty() {
        lines.push(Line::from(Span::styled(" nothing to switch", thought())));
    }
    (lines, selected_line)
}

fn help() -> Vec<Line<'static>> {
    KEYS.iter()
        .map(|(keys, what)| {
            Line::from(vec![
                Span::styled(format!(" {keys:>14}  "), bold(SUN)),
                Span::raw(*what),
            ])
        })
        .collect()
}

/// A row with something on the right, highlighted when selected. The right part is
/// placed by `fit` once the width is known.
fn selectable(left: Vec<Span<'static>>, right: Span<'static>, selected: bool) -> Line<'static> {
    let mut spans = left;
    spans.push(Span::raw("\t"));
    spans.push(right);
    let line = Line::from(spans);
    if selected {
        line.style(Style::new().bg(GLOW).add_modifier(Modifier::BOLD))
    } else {
        line
    }
}

/// Pushes the part after a tab to the right edge, clipping what does not fit.
fn fit(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| {
            let Some(tab) = line.spans.iter().position(|span| span.content == "\t") else {
                return line;
            };
            let style = line.style;
            let mut spans = line.spans;
            let right = spans.split_off(tab + 1);
            spans.pop();
            let left_width: usize = spans.iter().map(Span::width).sum();
            let right_text: String = right.iter().map(|span| span.content.as_ref()).collect();
            let right_style = right.first().map_or(Style::new(), |span| span.style);
            let room = width.saturating_sub(left_width + 3);
            let right_text = wrap::clip(&right_text, room);
            let gap = width.saturating_sub(left_width + right_text.width() + 1);
            spans.push(Span::raw(" ".repeat(gap)));
            spans.push(Span::styled(right_text, right_style));
            Line::from(spans).style(style)
        })
        .collect()
}

fn age(ms: i64) -> String {
    let minutes = ms.max(0) / 60_000;
    match minutes {
        0 => "now".into(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}
