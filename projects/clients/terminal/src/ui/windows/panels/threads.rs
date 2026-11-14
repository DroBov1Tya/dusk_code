//! /resume: the threads, with a preview of the chosen one.

use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};

use dusk_protocol::thread::MessageRole;

use super::frame::{age, ago, fit, selectable, window};
use crate::app::{App, ThreadList, paste};
use crate::ui::windows::{
    markdown,
    theme::{
        ACCENT, DOT, DUSK, ELLIPSIS, GUTTER, HAIRLINE, SUN_GLYPH, TEXT, TEXT_MUTED, USER_BAR, bold,
        dim, fg, thought,
    },
    wrap,
};

/// Messages shown at the end of a preview, after the one the thread began with.
pub const PREVIEW_LAST: usize = 4;
pub const PREVIEW_USER_LINES: usize = 3;
pub const PREVIEW_ANSWER_LINES: usize = 6;
/// Below this the preview leaves the whole window to the list.
pub const PREVIEW_MIN_WIDTH: u16 = 70;

/// Threads take most of the screen: the list on the left and the selected thread at a
/// glance on the right, how it began and how it stands, never its whole history.
pub fn draw_threads(frame: &mut Frame, screen: Rect, app: &App, list: &ThreadList) {
    let area = window(screen, 150);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(HAIRLINE))
        .title(Line::from(Span::styled(" threads ", bold(DUSK))))
        .title_bottom(
            Line::from(Span::styled(
                " up/down choose  enter open  y copy UID  n new  esc close ",
                dim(),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    // Keep the complete selected identifier visible even without the preview pane.
    let uid: Vec<Line> = list
        .threads
        .get(list.selected)
        .map(|thread| {
            wrap::chars(
                &format!("UID (y copy): {}", thread.id),
                inner.width.max(1) as usize,
            )
            .into_iter()
            .map(|line| Line::from(Span::styled(line, dim())))
            .collect()
        })
        .unwrap_or_default();
    let uid_height = (uid.len() as u16).min(inner.height);
    frame.render_widget(
        Paragraph::new(uid),
        Rect {
            height: uid_height,
            ..inner
        },
    );
    let inner = Rect {
        y: inner.y + uid_height,
        height: inner.height.saturating_sub(uid_height),
        ..inner
    };
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
    let bar = vec![Line::from(Span::styled(GUTTER, fg(HAIRLINE))); divider.height as usize];
    frame.render_widget(Paragraph::new(bar), divider);
    let right = Rect {
        x: divider.x + 2,
        width: inner.width.saturating_sub(list_width + 3),
        ..inner
    };
    let lines = preview(list, right.width as usize, right.height as usize);
    frame.render_widget(Paragraph::new(lines), right);
}

pub fn preview(list: &ThreadList, width: usize, height: usize) -> Vec<Line<'static>> {
    let Some(thread) = list.threads.get(list.selected) else {
        return Vec::new();
    };
    let now = dusk_protocol::system::now_ms();
    let title = thread.title.clone().unwrap_or_else(|| "no messages".into());
    let mut head = vec![Line::from(Span::styled(
        wrap::clip(&title, width),
        bold(TEXT),
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
pub fn message_lines((role, text): &(MessageRole, String), width: usize) -> Vec<Line<'static>> {
    let (mut lines, limit) = match role {
        MessageRole::User => {
            let lines = paste::labelled(text)
                .lines()
                .flat_map(|line| wrap::words(vec![Span::raw(line.to_owned())], width - 3, 0))
                .map(|spans| {
                    let bar = Span::styled(format!(" {USER_BAR} "), fg(TEXT_MUTED));
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

pub fn threads(list: &ThreadList, current: Option<&str>) -> Vec<Line<'static>> {
    if list.threads.is_empty() {
        return vec![Line::from(Span::styled(" no threads yet", thought()))];
    }
    let now = dusk_protocol::system::now_ms();
    list.threads
        .iter()
        .enumerate()
        .map(|(row, thread)| {
            let mark = if current == Some(thread.id.as_str()) {
                Span::styled(format!(" {SUN_GLYPH} "), fg(ACCENT))
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
