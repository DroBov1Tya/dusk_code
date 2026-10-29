//! The thread on screen. Text blocks get room around them; thinking, tool calls and
//! summaries fold into one line each and stack tightly, so the conversation stays in view.

use dusk_core::{providers::ToolCall, tools::ToolResult};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};

use super::{
    markdown,
    theme::{
        DONE, DOT, EMBER, FAILED, FOLDED, GLOW, GUTTER, ROSE, SPINNER, SUN, SUN_GLYPH, TEAL, TOOL,
        TOOL_RUNNING, UNFOLDED, USER_BAR, VIOLET, bold, dim, dusk, fg, thought,
    },
    wrap,
};
use crate::app::{App, Entry, Focus, Tone, TranscriptHit, compact_number};

/// Lines of a tool result shown when it is open.
const MAX_RESULT_LINES: usize = 40;

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    app.viewport = area.height as usize;
    app.transcript_area = Some(area);
    app.transcript_hits.clear();
    if app.transcript.entries.is_empty() {
        app.scroll = 0;
        app.max_scroll = 0;
        welcome(frame, area);
        return;
    }
    let width = area.width as usize;
    let (lines, starts) = layout(app, width);
    let height = area.height as usize;
    // The trailing blank line breathes only when it fits; it is not scrollable content.
    let total = lines.len().saturating_sub(1);
    app.max_scroll = if height == 0 {
        0
    } else {
        total.saturating_sub(height)
    };
    if let Focus::Browse(selected) = app.focus {
        keep_in_view(app, starts[selected], total, height);
    }
    app.scroll = app.scroll.min(app.max_scroll);
    let top = total.saturating_sub(height + app.scroll);
    for (index, start) in starts.into_iter().enumerate() {
        if app.transcript.is_foldable(index) && start >= top && start < top + height {
            app.transcript_hits.push(TranscriptHit {
                row: area.y + (start - top) as u16,
                left: area.x,
                right: area.right(),
                index,
            });
        }
    }
    let visible: Vec<Line> = lines.into_iter().skip(top).take(height).collect();
    frame.render_widget(Paragraph::new(visible), area);
}

/// Every entry as lines, and the line each entry starts on.
fn layout(app: &App, width: usize) -> (Vec<Line<'static>>, Vec<usize>) {
    let text_width = width.saturating_sub(4).max(10);
    let mut lines = Vec::new();
    let mut starts = Vec::new();
    let entries = &app.transcript.entries;
    let streaming = app.is_busy().then(|| entries.len().saturating_sub(1));
    for (index, entry) in entries.iter().enumerate() {
        let folded_run = index > 0 && is_meta(&entries[index - 1]) && is_meta(entry);
        if index > 0 && !folded_run {
            lines.push(Line::default());
        }
        starts.push(lines.len());
        let live = streaming == Some(index);
        let mut drawn = entry_lines(entry, text_width, live.then_some(app));
        if app.focus == Focus::Browse(index)
            && let Some(header) = drawn.first_mut()
        {
            *header = std::mem::take(header).style(Style::new().bg(GLOW));
        }
        lines.extend(drawn);
    }
    lines.push(Line::default());
    (lines, starts)
}

/// `live` is set while the entry is still being written.
fn entry_lines(entry: &Entry, width: usize, live: Option<&App>) -> Vec<Line<'static>> {
    match entry {
        Entry::User(text) => {
            let bar = || Span::styled(format!(" {USER_BAR} "), fg(ROSE));
            text.lines()
                .flat_map(|line| wrap::words(vec![Span::raw(line.to_owned())], width, 0))
                .map(|spans| Line::from([vec![bar()], spans].concat()))
                .collect()
        }
        Entry::Answer(text) => indent(markdown::render(text.trim_end(), width), "   "),
        Entry::Reasoning { text, open } => {
            let marker = live.map_or(fold_mark(*open), |app| spinner(app));
            let header = Line::from(vec![
                Span::styled(format!("   {marker} "), thought()),
                Span::styled("thinking", thought()),
                Span::styled(format!(" {DOT} ~{} tokens", tokens(text)), dim()),
            ]);
            folded(header, *open, || {
                let body = plain(text.trim(), width.saturating_sub(2), thought());
                indent(body, &format!("   {GUTTER} "))
            })
        }
        Entry::Tool { call, result, open } => tool(call, result.as_ref(), *open, width, live),
        Entry::Summary { text, open } => {
            let header = Line::from(vec![
                Span::styled(format!("   {} ", fold_mark(*open)), fg(VIOLET)),
                Span::styled("summary of the earlier conversation", fg(VIOLET)),
                Span::styled(format!(" {DOT} ~{} tokens", tokens(text)), dim()),
            ]);
            folded(header, *open, || {
                let body = markdown::render(text, width.saturating_sub(2));
                indent(body, &format!("   {GUTTER} "))
            })
        }
        Entry::Notice { text, tone } => {
            let (mark, style) = match tone {
                Tone::Info => (DOT, dim()),
                Tone::Error => (FAILED, fg(EMBER)),
            };
            let spans = vec![Span::styled(text.clone(), style)];
            wrap::words(spans, width.saturating_sub(2), 0)
                .into_iter()
                .enumerate()
                .map(|(row, spans)| {
                    let lead = if row == 0 {
                        format!("   {mark} ")
                    } else {
                        "     ".into()
                    };
                    Line::from([vec![Span::styled(lead, style)], spans].concat())
                })
                .collect()
        }
    }
}

fn tool(
    call: &ToolCall,
    result: Option<&ToolResult>,
    open: bool,
    width: usize,
    live: Option<&App>,
) -> Vec<Line<'static>> {
    let (mark, status) = match result {
        None => (
            Span::styled(format!("   {TOOL_RUNNING} "), fg(TEAL)),
            Span::styled(live.map_or("", spinner), fg(SUN)),
        ),
        Some(result) if result.is_error => (
            Span::styled(format!("   {TOOL} "), fg(EMBER)),
            Span::styled(format!("{FAILED} failed"), fg(EMBER)),
        ),
        Some(result) => (
            Span::styled(format!("   {TOOL} "), fg(TEAL)),
            Span::styled(format!("{DONE} {}", line_count(&result.content)), dim()),
        ),
    };
    let name = Span::styled(call.name.clone(), bold(TEAL));
    let room = width.saturating_sub(call.name.len() + status.width() + 6);
    let arguments = Span::styled(
        format!("  {}  ", wrap::clip(&preview(&call.arguments), room)),
        dim(),
    );
    let header = Line::from(vec![mark, name, arguments, status]);
    folded(header, open, || {
        let gutter = format!("   {GUTTER} ");
        let mut body = indent(
            verbatim(&pretty(&call.arguments), width - 2, dim()),
            &gutter,
        );
        if let Some(result) = result {
            let style = if result.is_error {
                fg(EMBER)
            } else {
                Style::new()
            };
            let all: Vec<&str> = result.content.lines().collect();
            let shown = all
                .iter()
                .take(MAX_RESULT_LINES)
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
            body.push(Line::from(vec![
                Span::styled(format!("   {GUTTER}"), dim()),
                Span::styled(format!(" {} returned", call.name), thought()),
            ]));
            body.extend(indent(verbatim(&shown, width - 2, style), &gutter));
            if all.len() > MAX_RESULT_LINES {
                let more = format!("{} more lines", all.len() - MAX_RESULT_LINES);
                body.push(Line::from(vec![
                    Span::styled(gutter, dim()),
                    Span::styled(more, dim()),
                ]));
            }
        }
        body
    })
}

fn folded(
    header: Line<'static>,
    open: bool,
    body: impl FnOnce() -> Vec<Line<'static>>,
) -> Vec<Line<'static>> {
    let mut lines = vec![header];
    if open {
        lines.extend(body());
    }
    lines
}

fn plain(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    text.lines()
        .flat_map(|line| wrap::words(vec![Span::styled(line.to_owned(), style)], width, 0))
        .map(Line::from)
        .collect()
}

/// Text whose spacing matters, such as JSON or a file: every space stays.
fn verbatim(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    text.lines()
        .flat_map(|line| wrap::chars(line, width))
        .map(|line| Line::from(Span::styled(line, style)))
        .collect()
}

fn indent(lines: Vec<Line<'static>>, lead: &str) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| {
            let mut spans = vec![Span::styled(lead.to_owned(), dim())];
            spans.extend(line.spans);
            Line::from(spans).style(line.style)
        })
        .collect()
}

fn is_meta(entry: &Entry) -> bool {
    matches!(entry, Entry::Reasoning { .. } | Entry::Tool { .. })
}

fn fold_mark(open: bool) -> &'static str {
    if open { UNFOLDED } else { FOLDED }
}

pub fn spinner(app: &App) -> &'static str {
    SPINNER[(app.frame / 3) % SPINNER.len()]
}

fn tokens(text: &str) -> String {
    compact_number(text.len().div_ceil(4) as u64)
}

fn line_count(text: &str) -> String {
    match text.lines().count() {
        0 => "empty".into(),
        1 => wrap::clip(text.trim(), 40),
        lines => format!("{lines} lines"),
    }
}

fn pretty(arguments: &serde_json::Value) -> String {
    serde_json::to_string_pretty(arguments).unwrap_or_default()
}

/// Scrolls just enough to show the selected entry.
fn keep_in_view(app: &mut App, start: usize, total: usize, height: usize) {
    let bottom = total.saturating_sub(app.scroll);
    let top = bottom.saturating_sub(height);
    if start < top {
        app.scroll = total.saturating_sub(start + height);
    } else if start >= bottom {
        app.scroll = total.saturating_sub(start + 1);
    }
}

/// A new thread: the sun on the horizon and what to do.
fn welcome(frame: &mut Frame, area: Rect) {
    let width = (area.width as usize).min(44);
    let horizon: Vec<Span> = (0..width)
        .map(|x| Span::styled("\u{2500}", fg(dusk(x as f64 / width as f64))))
        .collect();
    let lines = vec![
        Line::from(Span::styled(SUN_GLYPH, bold(SUN))),
        Line::from(horizon),
        Line::default(),
        Line::from(Span::styled("dusk", bold(VIOLET))),
        Line::default(),
        Line::from(Span::styled("Write a message to begin.", Style::new())),
        Line::from(Span::styled(
            "Ctrl+T opens a thread you had before, F1 shows every key.",
            dim(),
        )),
    ];
    let top = area.height.saturating_sub(lines.len() as u16) / 2;
    let area = Rect {
        y: area.y + top,
        height: area.height - top,
        ..area
    };
    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

/// What a tool call is about, in a few words: its command or path when it has one.
fn preview(arguments: &serde_json::Value) -> String {
    let text = |key: &str| arguments.get(key).and_then(|value| value.as_str());
    if let Some(task) = text("task") {
        let agent = text("agent").unwrap_or("general");
        return format!("{agent}: {}", one_line(task));
    }
    for key in ["command", "path", "search", "name"] {
        if let Some(value) = text(key) {
            return one_line(value);
        }
    }
    arguments.to_string()
}

pub fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
