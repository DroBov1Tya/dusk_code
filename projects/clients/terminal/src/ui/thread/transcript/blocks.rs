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
        TEXT_SOFT, TOOL, UNFOLDED, USER_BAR, bold, dim, fg, step_glyph, thought,
    },
    wrap,
};
use dusk_protocol::{
    thread::{JobResult, Tool, ToolState},
    tools::Answered,
};
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
            // The terminal does not play sound; it names the file that was sent.
            Part::Sound { path, .. } => lines.push(Line::from(vec![
                bar(),
                Span::styled("\u{266A} ", fg(TEXT_MUTED)),
                Span::styled(path.to_owned(), fg(TEXT_SOFT)),
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
    card(lines, width)
}

/// The agent's questions as the user answered them: one bordered block, a row for each
/// question with its answer dimmed under it, the rows parted by a dim rule.
pub(super) fn answered(answers: &[Answered], width: usize) -> Vec<Line<'static>> {
    let outer = (width + 3).max(8);
    let inner = outer - 4;
    let edge = || fg(TEXT_MUTED);
    let rule = |left: &str, right: &str| {
        let line = format!("{left}{}{right}", "\u{2500}".repeat(outer - 2));
        Line::from(Span::styled(line, edge()))
    };
    let text = |text: &str, style: Style| -> Vec<Line<'static>> {
        text.lines()
            .flat_map(|line| wrap::words(vec![Span::styled(line.to_owned(), style)], inner, 0))
            .map(|spans| {
                let pad = inner.saturating_sub(Line::from(spans.clone()).width());
                let mut line = vec![Span::styled("\u{2502} ", edge())];
                line.extend(spans);
                line.push(Span::raw(" ".repeat(pad)));
                line.push(Span::styled(" \u{2502}", edge()));
                Line::from(line)
            })
            .collect()
    };
    let mut lines = vec![rule("\u{256D}", "\u{256E}")];
    for (index, answer) in answers.iter().enumerate() {
        if index > 0 {
            lines.push(rule("\u{251C}", "\u{2524}"));
        }
        lines.extend(text(&answer.question, fg(TEXT)));
        for line in answer.lines() {
            lines.extend(text(&line, dim()));
        }
    }
    lines.push(rule("\u{2570}", "\u{256F}"));
    lines
}

/// A finished background job in one line; opened, the start of what it printed.
pub(super) fn job(result: &JobResult, open: bool, width: usize) -> Vec<Line<'static>> {
    let (mark, color) = match result.failed() {
        true => (FAILED, NEGATIVE),
        false => (DONE, POSITIVE),
    };
    let header = Line::from(vec![
        Span::styled(format!("   {} ", fold_mark(open)), fg(TEXT_MUTED)),
        Span::styled(result.title(), fg(TEXT_SOFT)),
        Span::styled(format!("  {mark}"), fg(color)),
    ]);
    folded(header, open, || {
        let body = verbatim(result.output.trim_end(), width.saturating_sub(2), dim());
        indent(body, &format!("   {GUTTER} "))
    })
}

/// Lines on the soft card of a sent message, which reaches the right edge: the bar, the
/// text and the air after it.
fn card(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
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
    tool: &Tool,
    open: bool,
    width: usize,
    live: Option<&App>,
) -> Vec<Line<'static>> {
    // The arguments as they were sent, when they were short enough to be kept whole.
    let arguments = serde_json::from_str::<serde_json::Value>(&tool.arguments).ok();
    // The mark says what kind of work the step is; its colour, how the step stands.
    let glyph = step_glyph(&tool.name);
    let (mark, status) = match tool.state {
        ToolState::Running => (
            Span::styled(format!("   {glyph} "), fg(ACCENT)),
            Span::styled(live.map_or("", spinner), fg(ACCENT)),
        ),
        ToolState::Failed => (
            Span::styled(format!("   {glyph} "), fg(NEGATIVE)),
            Span::styled(FAILED, fg(NEGATIVE)),
        ),
        ToolState::Done => (
            Span::styled(format!("   {glyph} "), fg(TEXT_MUTED)),
            Span::styled(DONE, fg(POSITIVE)),
        ),
    };
    // Said as a person would, the tool's own name once the step is open; work handed to
    // another agent carries the accent.
    let said = if open {
        tool.name.clone()
    } else {
        dusk_protocol::tools::phrase(&tool.name, &tool.call_id).to_owned()
    };
    let lead = if dusk_protocol::tools::is_agent_tool(&tool.name) {
        ACCENT
    } else {
        TEXT
    };
    let name = Span::styled(said, fg(lead));
    // A command says what it does in a few words; the command itself is in the body.
    let about = arguments
        .as_ref()
        .and_then(|arguments| arguments.get("description"))
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
        let shown = arguments.as_ref().map_or(tool.arguments.clone(), pretty);
        let mut body = indent(verbatim(&shown, width - 2, dim()), &gutter);
        if tool.state != ToolState::Running {
            let style = if tool.state == ToolState::Failed {
                fg(NEGATIVE)
            } else {
                Style::new()
            };
            let all: Vec<&str> = tool.output.lines().collect();
            let shown = all
                .iter()
                .take(MAX_RESULT_LINES)
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
            body.push(Line::from(vec![
                Span::styled(format!("   {GUTTER}"), dim()),
                Span::styled(format!(" {} returned", tool.name), thought()),
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
