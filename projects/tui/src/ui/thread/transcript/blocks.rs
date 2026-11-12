//! The blocks an entry is made of: a user message, a tool step, a fold, plain and verbatim text.

use super::lines::MAX_RESULT_LINES;
use super::spinner;
use crate::app::{
    App, compact_number,
    paste::{self, Part},
};
use crate::ui::text::format::one_line;
use crate::ui::thread::{
    loader::{Loader, agent_name, identity},
    markdown,
    theme::{
        ACCENT, DONE, DOT, FAILED, FOLDED, GUTTER, NEGATIVE, POSITIVE, RAISED, TEXT, TEXT_MUTED,
        TEXT_SOFT, TOOL, TOOL_RUNNING, UNFOLDED, USER_BAR, bold, dim, fg, thought,
    },
    wrap,
};
use dusk_protocol::{model::ToolCall, tools::ToolResult};
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

/// A message from an agent, under its name and in its color, as on its card.
pub(super) fn from_agent(
    thread_id: &str,
    text: &str,
    open: bool,
    width: usize,
) -> Vec<Line<'static>> {
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
pub(super) fn user(text: &str, open: bool, width: usize) -> Vec<Line<'static>> {
    let bar = || Span::styled(format!(" {USER_BAR} "), fg(TEXT_MUTED));
    let mut lines = vec![Line::from(bar())];
    for part in paste::split(text) {
        match part {
            Part::Text(text) => lines.extend(
                text.trim_matches('\n')
                    .lines()
                    .flat_map(|line| wrap::words(vec![Span::raw(line.to_owned())], width, 0))
                    .map(|spans| Line::from([vec![bar()], spans].concat())),
            ),
            Part::Source(name) => lines.push(Line::from(vec![
                bar(),
                Span::styled(format!("{TOOL} "), fg(TEXT_MUTED)),
                Span::styled(name.to_owned(), fg(TEXT_SOFT)),
            ])),
            Part::Pasted(pasted) => {
                lines.push(Line::from(vec![
                    bar(),
                    Span::styled(format!("{} ", fold_mark(open)), fg(TEXT_MUTED)),
                    Span::styled(paste::label(pasted), fg(TEXT_SOFT)),
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
                    let style = span.style.bg(RAISED);
                    span.style(style)
                })
                .collect();
            spans.push(Span::styled(" ".repeat(pad), Style::new().bg(RAISED)));
            Line::from(spans)
        })
        .collect()
}

pub(super) fn tool(
    call: &ToolCall,
    result: Option<&ToolResult>,
    open: bool,
    width: usize,
    live: Option<&App>,
) -> Vec<Line<'static>> {
    let (mark, status) = match result {
        None => (
            Span::styled(format!("   {TOOL_RUNNING} "), fg(ACCENT)),
            Span::styled(live.map_or("", spinner), fg(ACCENT)),
        ),
        Some(result) if result.is_error => (
            Span::styled(format!("   {TOOL} "), fg(NEGATIVE)),
            Span::styled(FAILED, fg(NEGATIVE)),
        ),
        Some(_) => (
            Span::styled(format!("   {TOOL} "), fg(TEXT_MUTED)),
            Span::styled(DONE, fg(POSITIVE)),
        ),
    };
    // Said as a person would, the tool's own name once the step is open; work handed to
    // another agent carries the accent.
    let said = if open {
        call.name.clone()
    } else {
        dusk_protocol::tools::phrase(&call.name, &call.id).to_owned()
    };
    let lead = if dusk_protocol::tools::is_agent_tool(&call.name) {
        ACCENT
    } else {
        TEXT
    };
    let name = Span::styled(said, fg(lead));
    // A command says what it does in a few words; the command itself is in the body.
    let about = call
        .arguments
        .get("description")
        .and_then(|text| text.as_str())
        .map(|text| {
            // Whatever is left after the mark, the name and the status, so the status
            // always shows.
            let used = mark.content.width() + name.content.width() + 2 + status.content.width();
            let room = width.saturating_sub(used + 2);
            format!("  {}", wrap::clip(&one_line(text), room.min(60)))
        })
        .filter(|about| about.trim().len() > 1)
        .unwrap_or_default();
    let header = Line::from(vec![
        mark,
        name,
        Span::styled(about, fg(TEXT_MUTED)),
        Span::raw("  "),
        status,
    ]);
    folded(header, open, || {
        let gutter = format!("   {GUTTER} ");
        let mut body = indent(
            verbatim(&pretty(&call.arguments), width - 2, dim()),
            &gutter,
        );
        if let Some(result) = result {
            let style = if result.is_error {
                fg(NEGATIVE)
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

pub(super) fn folded(
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

pub(super) fn plain(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    text.lines()
        .flat_map(|line| wrap::words(vec![Span::styled(line.to_owned(), style)], width, 0))
        .map(Line::from)
        .collect()
}

/// Text whose spacing matters, such as JSON or a file: every space stays.
pub(super) fn verbatim(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    text.lines()
        .flat_map(|line| wrap::chars(line, width))
        .map(|line| Line::from(Span::styled(line, style)))
        .collect()
}

pub(super) fn indent(lines: Vec<Line<'static>>, lead: &str) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| {
            let mut spans = vec![Span::styled(lead.to_owned(), dim())];
            spans.extend(line.spans);
            Line::from(spans).style(line.style)
        })
        .collect()
}

pub(super) fn fold_mark(open: bool) -> &'static str {
    if open { UNFOLDED } else { FOLDED }
}

pub(super) fn tokens(text: &str) -> String {
    compact_number(text.len().div_ceil(4) as u64)
}

pub(super) fn pretty(arguments: &serde_json::Value) -> String {
    serde_json::to_string_pretty(arguments).unwrap_or_default()
}
