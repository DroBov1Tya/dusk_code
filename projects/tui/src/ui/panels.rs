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

use dusk_core::state::MessageRole;

use super::{
    markdown,
    theme::{
        DOT, ELLIPSIS, GLOW, GUTTER, NIGHT, OFF, ON, ROSE, SUN, SUN_GLYPH, USER_BAR, VIOLET, bold,
        dim, fg, thought,
    },
    wrap,
};
use crate::app::{App, Panel, SettingKind, SettingList, ThreadList};

const KEYS: [(&str, &str); 20] = [
    ("Enter", "send; it waits if the agent is busy"),
    ("Alt+Enter", "send now, interrupting the agent"),
    ("Esc", "stop the agent"),
    ("Ctrl+J", "new line"),
    ("Up / Down", "earlier messages you sent"),
    ("Tab", "walk thinking, tools and summaries"),
    ("Enter / Space", "fold or unfold the selected one"),
    ("Left click", "fold or unfold thinking, tools and summaries"),
    ("Drag", "select text and copy it; Esc clears"),
    ("Hover", "copy a message, edit one you sent"),
    ("Ctrl+R", "fold or unfold all thinking"),
    ("PgUp / PgDn", "scroll the transcript; wheel works too"),
    ("Shift+PgUp/Dn", "scroll subagent cards"),
    ("Alt+PgUp/Dn", "scroll MCP tools"),
    ("Ctrl+T /resume", "threads, with a preview of each"),
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
        Panel::Threads(list) => return draw_threads(frame, screen, app, list),
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

/// Messages shown at the end of a preview, after the one the thread began with.
const PREVIEW_LAST: usize = 4;
const PREVIEW_USER_LINES: usize = 3;
const PREVIEW_ANSWER_LINES: usize = 6;
/// Below this the preview leaves the whole window to the list.
const PREVIEW_MIN_WIDTH: u16 = 70;

/// Threads take most of the screen: the list on the left and the selected thread at a
/// glance on the right, how it began and how it stands, never its whole history.
fn draw_threads(frame: &mut Frame, screen: Rect, app: &App, list: &ThreadList) {
    let width = screen.width.saturating_sub(6).min(150);
    let height = screen.height.saturating_sub(4).max(3);
    let area = Rect {
        x: screen.x + (screen.width - width) / 2,
        y: screen.y + (screen.height - height) / 2,
        width,
        height,
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(NIGHT))
        .title(Line::from(Span::styled(" threads ", bold(VIOLET))))
        .title_bottom(
            Line::from(Span::styled(
                " up/down choose  enter open  n new  esc close ",
                dim(),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let with_preview = inner.width >= PREVIEW_MIN_WIDTH;
    let list_width = if with_preview {
        inner.width * 2 / 5
    } else {
        inner.width
    };
    let left = Rect {
        width: list_width,
        ..inner
    };
    let rows = fit(threads(list, app.thread_id.as_deref()), left.width as usize);
    let skip = (list.selected + 1).saturating_sub(left.height as usize);
    frame.render_widget(Paragraph::new(rows).scroll((skip as u16, 0)), left);
    if !with_preview {
        return;
    }
    let divider = Rect {
        x: inner.x + list_width,
        width: 1,
        ..inner
    };
    let bar = vec![Line::from(Span::styled(GUTTER, fg(NIGHT))); divider.height as usize];
    frame.render_widget(Paragraph::new(bar), divider);
    let right = Rect {
        x: divider.x + 2,
        width: inner.width.saturating_sub(list_width + 3),
        ..inner
    };
    let lines = preview(list, right.width as usize, right.height as usize);
    frame.render_widget(Paragraph::new(lines), right);
}

fn preview(list: &ThreadList, width: usize, height: usize) -> Vec<Line<'static>> {
    let Some(thread) = list.threads.get(list.selected) else {
        return Vec::new();
    };
    let now = now_ms();
    let title = thread.title.clone().unwrap_or_else(|| "no messages".into());
    let mut head = vec![Line::from(Span::styled(
        wrap::clip(&title, width),
        bold(SUN),
    ))];
    let loaded = list
        .preview
        .as_ref()
        .filter(|preview| preview.thread_id == thread.id);
    let Some(preview) = loaded else {
        head.push(Line::from(Span::styled("loading", thought())));
        return head;
    };
    let meta = format!(
        "started {} {DOT} active {} {DOT} {} messages {DOT} {} tool calls",
        ago(now - thread.created_at),
        ago(now - thread.active_at),
        preview.messages.len(),
        preview.tool_calls,
    );
    head.push(Line::from(Span::styled(wrap::clip(&meta, width), dim())));
    head.push(Line::default());
    let Some((first, rest)) = preview.messages.split_first() else {
        head.push(Line::from(Span::styled("no messages yet", thought())));
        return head;
    };
    head.extend(message_lines(first, width));
    let last = &rest[rest.len().saturating_sub(PREVIEW_LAST)..];
    let between = rest.len() - last.len();
    let mut tail = Vec::new();
    if between > 0 {
        tail.push(Line::default());
        let skipped = format!("   {DOT} {between} more {DOT}");
        tail.push(Line::from(Span::styled(skipped, dim())));
    }
    for message in last {
        tail.push(Line::default());
        tail.extend(message_lines(message, width));
    }
    // The newest messages matter most, so a short window drops the older ones first.
    let room = height.saturating_sub(head.len());
    let tail = tail.split_off(tail.len().saturating_sub(room));
    head.extend(tail);
    head
}

/// A user message with its bar, or an answer as markdown, cut to a few lines.
fn message_lines((role, text): &(MessageRole, String), width: usize) -> Vec<Line<'static>> {
    let (mut lines, limit) = match role {
        MessageRole::User => {
            let lines = text
                .lines()
                .flat_map(|line| wrap::words(vec![Span::raw(line.to_owned())], width - 3, 0))
                .map(|spans| {
                    let bar = Span::styled(format!(" {USER_BAR} "), fg(ROSE));
                    Line::from([vec![bar], spans].concat())
                })
                .collect::<Vec<_>>();
            (lines, PREVIEW_USER_LINES)
        }
        _ => {
            let lines = markdown::render(text.trim(), width - 3)
                .into_iter()
                .map(|line| {
                    let mut spans = vec![Span::raw("   ")];
                    spans.extend(line.spans);
                    Line::from(spans)
                })
                .collect::<Vec<_>>();
            (lines, PREVIEW_ANSWER_LINES)
        }
    };
    if lines.len() > limit {
        lines.truncate(limit);
        lines.push(Line::from(Span::styled(format!("   {ELLIPSIS}"), dim())));
    }
    lines
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

fn ago(ms: i64) -> String {
    match age(ms).as_str() {
        "now" => "just now".into(),
        age => format!("{age} ago"),
    }
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
