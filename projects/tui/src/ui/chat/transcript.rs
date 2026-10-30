//! The thread on screen. Text blocks get room around them; thinking, tool calls and
//! summaries fold into one line each and stack tightly, so the conversation stays in view.

use dusk_core::{providers::ToolCall, tools::ToolResult};
use ratatui::{
    Frame,
    layout::{Alignment, Rect, Size},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use unicode_width::UnicodeWidthStr;

use super::{
    loader::{Loader, agent_name, identity},
    markdown,
    theme::{
        DONE, DOT, EMBER, FAILED, FOLDED, GLOW, GUTTER, HAZE, ORANGE, ROSE, SELECTED, SPINNER, SUN,
        SUN_GLYPH, TEAL, TOOL, TOOL_RUNNING, UNFOLDED, USER_BAR, VIOLET, bold, dim, dusk, fg,
        thought,
    },
    wrap,
};
use crate::app::{
    App, Button, ButtonHit, Entry, Focus, ImageSpot, Layout, ShownPreview, TEXT_COLUMN, Tone,
    Transcript, TranscriptHit, compact_number,
    paste::{self, Part},
};

/// Image cards sit this far in, under their tool call.
const CARD_INDENT: &str = "   ";
/// Lines of a tool result shown when it is open.
const MAX_RESULT_LINES: usize = 40;

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    app.viewport = area.height as usize;
    app.transcript_area = Some(area);
    app.transcript_hits.clear();
    app.previews.clear();
    if app.transcript.entries.is_empty() {
        app.scroll = 0;
        app.max_scroll = 0;
        welcome(frame, area);
        return;
    }
    let width = area.width as usize;
    let browsed = match app.focus {
        Focus::Browse(index) => Some(index),
        Focus::Input => None,
    };
    let last = app.transcript.entries.len() - 1;
    refresh_layout(app, width, browsed.filter(|&index| index < last));
    let Some(layout) = &app.layout else {
        return;
    };
    // Only the last entry can be in the middle of being written; it is drawn every frame.
    let earlier = layout.lines.len();
    let mut tail = Placed::default();
    let live = app.is_busy().then_some(&*app);
    let last_browsed = browsed.filter(|&index| index == last);
    append_entry(&app.transcript, last, width, live, last_browsed, &mut tail);
    let Placed {
        lines: mut tail,
        starts: tail_starts,
        images: tail_images,
    } = tail;
    tail.push(Line::default());
    let mut starts = layout.starts.clone();
    starts.extend(tail_starts.iter().map(|start| start + earlier));
    let height = area.height as usize;
    // The trailing blank line breathes only when it fits; it is not scrollable content.
    let total = (earlier + tail.len()).saturating_sub(1);
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
    for (index, &start) in starts.iter().enumerate() {
        if !app.transcript.is_foldable(index) {
            continue;
        }
        // A folded entry opens from its header; a message with a paste, from any row.
        let end = match app.transcript.entries[index] {
            Entry::User { .. } => starts.get(index + 1).copied().unwrap_or(total),
            _ => start + 1,
        };
        for line in start.max(top)..end.min(top + height) {
            app.transcript_hits.push(TranscriptHit {
                row: area.y + (line - top) as u16,
                left: area.x,
                right: area.right(),
                index,
            });
        }
    }
    let visible: Vec<Line> = (top..(top + height).min(earlier + tail.len()))
        .map(|line| {
            match app
                .layout
                .as_ref()
                .and_then(|layout| layout.lines.get(line))
            {
                Some(line) => line.clone(),
                None => tail[line - earlier].clone(),
            }
        })
        .collect();
    // The rows of each preview that show; the rest are above or below the thread.
    let earlier_images = app
        .layout
        .as_ref()
        .map(|layout| layout.images.clone())
        .unwrap_or_default();
    app.previews = earlier_images
        .into_iter()
        .chain(tail_images.into_iter().map(|spot| ImageSpot {
            line: spot.line + earlier,
            ..spot
        }))
        .filter_map(|spot| {
            let from = spot.line.max(top);
            let to = (spot.line + spot.height as usize).min(top + height);
            (from < to).then(|| ShownPreview {
                area: Rect {
                    x: area.x + spot.column,
                    y: area.y + (from - top) as u16,
                    width: spot.width.min(area.width.saturating_sub(spot.column)),
                    height: (to - from) as u16,
                },
                card: Size::new(spot.width, spot.height),
                skip: (from - spot.line) as u16,
                artifact: spot.artifact,
            })
        })
        .collect();
    app.drawn_top = top;
    app.tail_text = tail.iter().map(plain_text).collect();
    app.entry_lines = starts
        .iter()
        .zip(starts.iter().skip(1).chain([&total]))
        .map(|(&from, &to)| (from, to))
        .collect();
    frame.render_widget(Paragraph::new(visible), area);
    highlight(frame, area, app);
    buttons(frame, area, app);
}

fn plain_text(line: &Line) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// Marks the selected text on screen, up to where each line's text ends.
fn highlight(frame: &mut Frame, area: Rect, app: &App) {
    let Some(selection) = app.selection else {
        return;
    };
    let (start, end) = selection.ordered();
    let text_left = area.x + TEXT_COLUMN;
    for row in 0..area.height {
        let line = app.drawn_top + row as usize;
        if line < start.0 || line > end.0 {
            continue;
        }
        let drawn = app.drawn_text(line).map_or(0, |text| text.width()) as u16;
        let from = if line == start.0 { start.1 } else { area.x }.max(text_left);
        let to = if line == end.0 {
            end.1 + 1
        } else {
            area.right()
        }
        .min(area.x + drawn);
        let buffer = frame.buffer_mut();
        for x in from..to {
            buffer[(x, area.y + row)].set_bg(SELECTED);
        }
    }
}

/// Buttons on the first row in view of the entry under the mouse, or the browsed one.
fn buttons(frame: &mut Frame, area: Rect, app: &mut App) {
    app.button_hits.clear();
    let browsed = match app.focus {
        Focus::Browse(index) => Some(index),
        Focus::Input => None,
    };
    let Some(index) = app.hover.or(browsed) else {
        return;
    };
    let offered: &[(Button, &str, Color)] = match app.transcript.entries.get(index) {
        Some(Entry::Notice { .. }) | None => &[],
        Some(_) if app.transcript.is_hidden(index) => &[(Button::Copy, " copy ", TEAL)],
        Some(Entry::User { .. }) => &[
            (Button::Hide, " hide ", HAZE),
            (Button::Edit, " edit ", ROSE),
            (Button::Copy, " copy ", TEAL),
        ],
        Some(_) => &[
            (Button::Hide, " hide ", HAZE),
            (Button::Copy, " copy ", TEAL),
        ],
    };
    let Some(&(from, to)) = app.entry_lines.get(index) else {
        return;
    };
    let line = from.max(app.drawn_top);
    if offered.is_empty() || line >= to || line >= app.drawn_top + area.height as usize {
        return;
    }
    let row = area.y + (line - app.drawn_top) as u16;
    let mut right = area.right();
    for &(button, label, color) in offered.iter().rev() {
        let width = label.width() as u16;
        let Some(left) = right.checked_sub(width + 1).filter(|&left| left >= area.x) else {
            break;
        };
        let place = Rect::new(left, row, width + 1, 1);
        let shown = Line::from(vec![
            Span::raw(" "),
            Span::styled(label, fg(color).bg(GLOW)),
        ]);
        frame.render_widget(Paragraph::new(shown), place);
        app.button_hits.push(ButtonHit {
            row,
            left: left + 1,
            right: left + 1 + width,
            index,
            button,
        });
        right = left;
    }
}

/// Lays out every entry but the last again, when the transcript, the width or the browsed
/// entry changed since the last frame.
fn refresh_layout(app: &mut App, width: usize, browsed: Option<usize>) {
    let revision = app.transcript.revision();
    let current = app.layout.as_ref().is_some_and(|layout| {
        layout.width == width && layout.revision == revision && layout.browsed == browsed
    });
    if current {
        return;
    }
    let mut placed = Placed::default();
    for index in 0..app.transcript.entries.len().saturating_sub(1) {
        append_entry(&app.transcript, index, width, None, browsed, &mut placed);
    }
    let Placed {
        lines,
        starts,
        images,
    } = placed;
    app.layout = Some(Layout {
        width,
        revision,
        browsed,
        text: lines.iter().map(plain_text).collect(),
        lines,
        starts,
        images,
    });
}

/// Every entry of a thread as lines, and the line each entry starts on. With `live`,
/// the last entry is still being written; `browsed` is highlighted.
pub fn thread_lines(
    transcript: &Transcript,
    width: usize,
    live: Option<&App>,
    browsed: Option<usize>,
) -> (Vec<Line<'static>>, Vec<usize>) {
    // Agent windows keep the half blocks; their pictures are not placed.
    let mut placed = Placed::default();
    for index in 0..transcript.entries.len() {
        let writing = live.filter(|_| index + 1 == transcript.entries.len());
        append_entry(transcript, index, width, writing, browsed, &mut placed);
    }
    placed.lines.push(Line::default());
    (placed.lines, placed.starts)
}

/// Entries laid out: their lines, the line each starts on, and their image previews.
#[derive(Default)]
struct Placed {
    lines: Vec<Line<'static>>,
    starts: Vec<usize>,
    images: Vec<ImageSpot>,
}

/// Adds the entry at `index`, after a blank line unless it continues a folded run.
fn append_entry(
    transcript: &Transcript,
    index: usize,
    width: usize,
    live: Option<&App>,
    browsed: Option<usize>,
    placed: &mut Placed,
) {
    let text_width = width.saturating_sub(4).max(10);
    let entries = &transcript.entries;
    let entry = &entries[index];
    let folded_run = index > 0 && is_meta(&entries[index - 1]) && is_meta(entry);
    if index > 0 && !folded_run {
        placed.lines.push(Line::default());
    }
    placed.starts.push(placed.lines.len());
    let mut previews = Vec::new();
    let mut drawn = if transcript.is_hidden(index) {
        let text = transcript.text(index).unwrap_or_default();
        vec![hidden_line(entry, &text)]
    } else {
        let mut lines = entry_lines(entry, text_width, live, ends_turn(entries, index));
        if transcript.images.enabled()
            && let Entry::Tool {
                call,
                result: Some(result),
                ..
            } = entry
            && call.name == "display_image"
            && !result.is_error
        {
            for artifact in &result.artifacts {
                let (card, preview) =
                    super::super::text::image_card::render(transcript, artifact, text_width);
                if let Some(preview) = preview {
                    previews.push((lines.len() + preview.row, preview, artifact.clone()));
                }
                lines.extend(indent(card, CARD_INDENT));
            }
        }
        lines
    };
    if browsed == Some(index)
        && let Some(header) = drawn.first_mut()
    {
        *header = std::mem::take(header).style(Style::new().bg(GLOW));
    }
    let first = placed.lines.len();
    placed.images.extend(
        previews
            .into_iter()
            .map(|(row, preview, artifact)| ImageSpot {
                line: first + row,
                column: CARD_INDENT.len() as u16,
                width: preview.width,
                height: preview.height,
                artifact,
            }),
    );
    placed.lines.extend(drawn);
}

/// What a hidden entry leaves: what it was and how long, to show it again.
fn hidden_line(entry: &Entry, text: &str) -> Line<'static> {
    let what = match entry {
        Entry::User { .. } => "your message",
        Entry::Answer(_) => "answer",
        Entry::Reasoning { .. } => "thinking",
        Entry::Tool { .. } => "tool call",
        Entry::Summary { .. } => "summary",
        Entry::FromAgent { .. } => "agent message",
        Entry::Notice { .. } => "notice",
    };
    let lines = text.lines().count().max(1);
    Line::from(vec![
        Span::styled(format!("   {FOLDED} "), dim()),
        Span::styled(format!("{what} hidden {DOT} {lines} lines"), dim()),
    ])
}

/// A message from an agent, under its name and in its color, as on its card.
fn from_agent(thread_id: &str, text: &str, open: bool, width: usize) -> Vec<Line<'static>> {
    let seed = identity(thread_id);
    let color = Loader::new(seed).color();
    let header = Line::from(vec![
        Span::styled(format!("   {} ", fold_mark(open)), fg(color)),
        Span::styled(agent_name(thread_id), bold(color)),
        Span::styled(
            format!(" {DOT} message {DOT} {} lines", text.lines().count()),
            dim(),
        ),
    ]);
    folded(header, open, || {
        let body = markdown::render(text.trim(), width.saturating_sub(2));
        indent(body, &format!("   {GUTTER} "))
    })
}

/// A sent message on a soft card behind its bar, with a row of air above and below;
/// each pasted block is one line until opened.
fn user(text: &str, open: bool, width: usize) -> Vec<Line<'static>> {
    let bar = || Span::styled(format!(" {USER_BAR} "), fg(ROSE));
    let mut lines = vec![Line::from(bar())];
    for part in paste::split(text) {
        match part {
            Part::Text(text) => lines.extend(
                text.trim_matches('\n')
                    .lines()
                    .flat_map(|line| wrap::words(vec![Span::raw(line.to_owned())], width, 0))
                    .map(|spans| Line::from([vec![bar()], spans].concat())),
            ),
            Part::Pasted(pasted) => {
                lines.push(Line::from(vec![
                    bar(),
                    Span::styled(format!("{} ", fold_mark(open)), fg(TEAL)),
                    Span::styled(paste::label(pasted), fg(TEAL)),
                ]));
                if open {
                    let gutter = || Span::styled(format!("{GUTTER} "), dim());
                    lines.extend(
                        plain(pasted, width.saturating_sub(2), Style::new())
                            .into_iter()
                            .map(|line| Line::from([vec![bar(), gutter()], line.spans].concat())),
                    );
                }
            }
        }
    }
    lines.push(Line::from(bar()));
    // The card reaches the right edge: the bar, the text and the air after it.
    let full = width + 3;
    lines
        .into_iter()
        .map(|line| {
            let pad = full.saturating_sub(line.width());
            let mut spans: Vec<Span> = line
                .spans
                .into_iter()
                .map(|span| {
                    let style = span.style.bg(GLOW);
                    span.style(style)
                })
                .collect();
            spans.push(Span::styled(" ".repeat(pad), Style::new().bg(GLOW)));
            Line::from(spans)
        })
        .collect()
}

/// Whether the answer at `index` is the last word of its turn: no tool call follows it.
fn ends_turn(entries: &[Entry], index: usize) -> bool {
    let next = entries[index + 1..]
        .iter()
        .find(|entry| !matches!(entry, Entry::Reasoning { .. } | Entry::Notice { .. }));
    matches!(entries[index], Entry::Answer(_)) && !matches!(next, Some(Entry::Tool { .. }))
}

/// `live` is set while the entry is still being written.
fn entry_lines(
    entry: &Entry,
    width: usize,
    live: Option<&App>,
    ends_turn: bool,
) -> Vec<Line<'static>> {
    match entry {
        Entry::User { text, open } => user(text, *open, width),
        Entry::Answer(text) if ends_turn => markdown::render(text.trim_end(), width)
            .into_iter()
            .map(|line| {
                let mut spans = vec![Span::styled(format!(" {USER_BAR} "), fg(ORANGE))];
                spans.extend(line.spans);
                Line::from(spans).style(line.style)
            })
            .collect(),
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
        Entry::FromAgent {
            thread_id,
            text,
            open,
        } => from_agent(thread_id, text, *open, width),
        Entry::Notice { text, tone, .. } => {
            let (mark, style) = match tone {
                Tone::Info => (DOT, dim()),
                Tone::Error => (FAILED, fg(EMBER)),
            };
            // Each line of a notice wraps on its own, so a line break stays a line break.
            text.lines()
                .flat_map(|line| {
                    let spans = vec![Span::styled(line.to_owned(), style)];
                    wrap::words(spans, width.saturating_sub(2), 0)
                })
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
    let arguments = Span::styled(format!("  {}  ", wrap::clip(&preview(call), room)), dim());
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
fn preview(call: &ToolCall) -> String {
    let arguments = &call.arguments;
    let text = |key: &str| arguments.get(key).and_then(|value| value.as_str());
    match call.name.as_str() {
        "ask_agent" => {
            let agent = text("thread").or(text("agent")).unwrap_or("general");
            return format!("{agent}: {}", one_line(text("task").unwrap_or_default()));
        }
        "tasks" => {
            let action = text("action").unwrap_or_default();
            let about = text("task").or(text("id")).unwrap_or_default();
            return one_line(&format!("{action} {about}"));
        }
        _ => {}
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
