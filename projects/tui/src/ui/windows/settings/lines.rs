//! The rows and lines the list is made of.

use super::Row;
use crate::app::{AgentRow, Input};
use crate::ui::windows::{
    theme::{
        ACCENT, FAILED, HAIRLINE, NEGATIVE, OFF, ON, POSITIVE, RAISED, SELECTED, SURFACE, TEXT,
        TEXT_MUTED, TEXT_SOFT, bold, dim, fg,
    },
    wrap,
};
use dusk_protocol::{
    catalog::{SkillInfo, SkillKind},
    config::{McpInfo, McpState, McpToolInfo},
};
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

pub(super) const ROOT: &str = "\u{25C9}";

pub(super) const TRACE: &str = "\u{2503}  ";

pub(super) const BRANCH: &str = "\u{2523}\u{2501}\u{2501}";

pub(super) const LAST_BRANCH: &str = "\u{2517}\u{2501}\u{2501}";

pub(super) fn memory_size_row(label: &'static str, chars: usize, width: usize) -> Row {
    Row {
        lines: vec![right(
            vec![Span::raw(label)],
            Span::styled(format!("− {chars} +  "), fg(TEXT)),
            width,
        )],
        switch: None,
        lead: 0,
    }
}

/// The value being edited has its own row so the caret remains visible in narrow windows.
/// These editors accept ASCII only, so byte offsets are also terminal cell offsets.
pub(super) fn manual_context_line(input: &Input, width: usize) -> Line<'static> {
    const INDENT: usize = 4;
    let available = width.saturating_sub(INDENT).max(1);
    let start = input.cursor.saturating_sub(available.saturating_sub(1));
    let selection = input.selection();
    let mut spans = vec![Span::raw(" ".repeat(INDENT))];
    for at in start..(start + available).min(input.text.len() + 1) {
        let cell = input
            .text
            .as_bytes()
            .get(at)
            .map_or(' ', |byte| *byte as char);
        let selected = selection.is_some_and(|(from, to)| at >= from && at < to);
        let style = if at == input.cursor {
            fg(SURFACE).bg(ACCENT)
        } else if selected {
            fg(ACCENT).bg(SELECTED)
        } else {
            fg(ACCENT)
        };
        spans.push(Span::styled(cell.to_string(), style));
    }
    Line::from(spans)
}

/// A tool of an open server, with its switch and the first line the server gave of it.
pub(super) fn mcp_tool_row(tool: &McpToolInfo, width: usize) -> Row {
    let short = tool
        .name
        .split_once("__")
        .map_or(tool.name.as_str(), |(_, tool)| tool);
    let name = Span::styled(
        format!("      {short}"),
        if tool.enabled { Style::new() } else { dim() },
    );
    let mut row = switch_row(vec![name], tool.enabled, width);
    let about = tool.description.lines().next().unwrap_or("").trim();
    if !about.is_empty() {
        row.lines.push(Line::from(Span::styled(
            wrap::clip(&format!("        {about}"), width),
            dim(),
        )));
    }
    row
}

/// The name, where it runs and how it stands, with the switch; then its command line or
/// URL, or why it failed.
pub(super) fn mcp_row(server: &McpInfo, open: bool, width: usize) -> Row {
    let on = server.state != McpState::Off;
    let (state, color) = match &server.state {
        McpState::Off => ("off".to_owned(), HAIRLINE),
        McpState::Starting => ("starting".to_owned(), ACCENT),
        McpState::Ready(_) => {
            let on = server.tools.iter().filter(|tool| tool.enabled).count();
            let mark = if open { "\u{25BE}" } else { "\u{25B8}" };
            match server.tools.len() {
                all if on == all => (format!("{all} tools {mark}"), POSITIVE),
                all => (format!("{on}/{all} tools {mark}"), ACCENT),
            }
        }
        McpState::Failed(_) => ("failed".to_owned(), NEGATIVE),
    };
    let place = if server.remote { "remote" } else { "local" };
    let first = vec![
        Span::styled(
            format!("  {}", server.name),
            if on { fg(TEXT) } else { dim() },
        ),
        Span::raw(" "),
        Span::styled(format!(" {place} "), fg(TEXT_SOFT).bg(RAISED)),
        Span::raw(" "),
        Span::styled(format!(" {state} "), fg(color).bg(RAISED)),
    ];
    let mut row = switch_row(first, on, width);
    let (text, style) = match &server.state {
        McpState::Failed(error) => (format!("{FAILED} {error}"), fg(NEGATIVE)),
        _ => (server.target.clone(), dim()),
    };
    // A failure is shown in full; an address stays on one line.
    match &server.state {
        McpState::Failed(_) => row.lines.extend(indented(&text, style, "    ", width)),
        _ => row.lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(wrap::clip(&text, width.saturating_sub(5)), style),
        ])),
    }
    row
}

/// `text` wrapped by words under `lead`, every line of it.
pub(super) fn indented(text: &str, style: Style, lead: &str, width: usize) -> Vec<Line<'static>> {
    let room = width.saturating_sub(lead.width() + 1).max(1);
    text.lines()
        .flat_map(|line| wrap::words(vec![Span::styled(line.to_owned(), style)], room, 0))
        .map(|spans| Line::from([vec![Span::raw(lead.to_owned())], spans].concat()))
        .collect()
}

/// The name and tags with the switch, then the description, or why it cannot be used.
pub(super) fn skill_row(skill: &SkillInfo, wide: bool, width: usize) -> Row {
    let mut first = vec![Span::styled(
        format!("  {}", skill.name),
        if skill.enabled { fg(TEXT) } else { dim() },
    )];
    let mut tags = vec![(skill.kind.name().to_owned(), kind_color(skill.kind))];
    tags.extend(skill.source.iter().map(|source| (source.clone(), ACCENT)));
    if wide {
        tags.extend(skill.tags.iter().map(|tag| (tag.clone(), TEXT_MUTED)));
    }
    for (tag, color) in tags {
        first.push(Span::raw(" "));
        first.push(Span::styled(format!(" {tag} "), fg(color).bg(RAISED)));
    }
    let mut row = switch_row(first, skill.enabled, width);
    let (text, style) = match &skill.problem {
        Some(problem) => (format!("{FAILED} {problem}"), fg(NEGATIVE)),
        None => (skill.description.clone(), dim()),
    };
    let room = width.saturating_sub(5);
    row.lines.push(Line::from(vec![
        Span::raw("    "),
        Span::styled(wrap::clip(&text, room), style),
    ]));
    row
}

pub(super) fn kind_color(kind: SkillKind) -> ratatui::style::Color {
    match kind {
        SkillKind::Builtin => TEXT_MUTED,
        SkillKind::Own => TEXT_SOFT,
        SkillKind::Other => ACCENT,
    }
}

pub(super) fn agent_row(agent: &AgentRow, width: usize) -> Row {
    if agent.depth == 0 {
        return Row {
            lines: vec![Line::from(vec![
                Span::styled(format!("  {ROOT} "), bold(TEXT)),
                Span::styled(agent.name.clone(), bold(TEXT)),
                Span::styled(format!("  {}", agent.description), dim()),
            ])],
            switch: None,
            lead: 0,
        };
    }
    let trace = if agent.active { TEXT_MUTED } else { HAIRLINE };
    let mut left = vec![Span::raw("  ")];
    for &goes_on in &agent.rails {
        left.push(Span::styled(
            if goes_on { TRACE } else { "   " },
            fg(HAIRLINE),
        ));
    }
    let branch = if agent.last { LAST_BRANCH } else { BRANCH };
    left.push(Span::styled(branch, fg(trace)));
    let (pad, pad_color) = match (agent.enabled, agent.active) {
        (true, true) => (ON, POSITIVE),
        (true, false) => (ON, ACCENT),
        (false, _) => (OFF, NEGATIVE),
    };
    left.push(Span::styled(format!("{pad} "), fg(pad_color)));
    left.push(Span::styled(
        agent.name.clone(),
        if agent.active { fg(TEXT) } else { dim() },
    ));
    if let Some(version) = agent.version {
        left.push(Span::styled(format!(" v{version}"), fg(TEXT_MUTED)));
    }
    if !agent.links.is_empty() {
        left.push(Span::styled(
            format!("  asks {}", agent.links.join(", ")),
            fg(TEXT_SOFT),
        ));
    }
    left.push(Span::styled(format!("  {}", agent.description), dim()));
    switch_row(left, agent.enabled, width)
}

/// A row with a dim line under it that says what it does.
pub(super) fn described(mut row: Row, about: &str, width: usize) -> Row {
    row.lines
        .push(Line::from(Span::styled(wrap::clip(about, width), dim())));
    row
}

pub(super) fn switch_row(left: Vec<Span<'static>>, on: bool, width: usize) -> Row {
    let label = if on { " on  " } else { " off " };
    let switch = Span::styled(
        label,
        if on {
            fg(POSITIVE).bg(RAISED)
        } else {
            fg(HAIRLINE).bg(RAISED)
        },
    );
    let room = width.saturating_sub(label.len() + 2);
    let line = right(clip_spans(left, room), switch, width.saturating_sub(1));
    let to = width.saturating_sub(1) as u16;
    Row {
        lines: vec![line],
        switch: Some((to.saturating_sub(label.len() as u16), to)),
        lead: 0,
    }
}

/// Bytes in the largest unit that keeps the number at least 1.
pub(super) fn bytes(count: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if count < 1024 {
        return format!("{count} B");
    }
    let mut value = count as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

pub(super) fn note(text: &str) -> Row {
    Row {
        lines: vec![Line::from(Span::styled(text.to_owned(), dim()))],
        switch: None,
        lead: 0,
    }
}

/// `left` with `end` pushed to the right edge of `width`.
pub(super) fn right(left: Vec<Span<'static>>, end: Span<'static>, width: usize) -> Line<'static> {
    let used: usize = left.iter().map(|span| span.content.width()).sum();
    let gap = width.saturating_sub(used + end.content.width());
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(end);
    Line::from(spans)
}

pub(super) fn clip_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let mut left = width;
    let mut clipped = Vec::new();
    for span in spans {
        if left == 0 {
            break;
        }
        let text = wrap::clip(&span.content, left);
        left = left.saturating_sub(text.width());
        clipped.push(Span::styled(text, span.style));
    }
    clipped
}
