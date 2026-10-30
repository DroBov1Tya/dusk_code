//! The settings window. Agents are drawn as a circuit: the chat is the source pad, each
//! agent a pad on the trace from the one that may ask it, lit while it can be asked.
//! Skills open beside the list: what their front matter says on top, the raw file below.

use dusk_core::{
    mcp::{McpInfo, McpState},
    skills::{SkillInfo, SkillKind},
};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::{
    input,
    panels::window,
    theme::{DOT, EMBER, FAILED, GLOW, NIGHT, OFF, ON, ROSE, SUN, TEAL, VIOLET, bold, dim, fg},
    wrap,
};
use crate::app::{
    AgentRow, App, FileButton, FileEditor, HitTarget, Panel, SettingCategory, SettingsHit,
    SettingsView,
};

const ROOT: &str = "\u{25C9}";
const TRACE: &str = "\u{2503}  ";
const BRANCH: &str = "\u{2523}\u{2501}\u{2501}";
const LAST_BRANCH: &str = "\u{2517}\u{2501}\u{2501}";
/// Rows the front matter box may take at most.
const METADATA_ROWS: u16 = 8;

/// A list item: its lines, and where its switch is on the first of its own lines, as
/// columns from the row's start. The first `lead` lines head a group and are not part
/// of the item.
struct Row {
    lines: Vec<Line<'static>>,
    switch: Option<(u16, u16)>,
    lead: usize,
}

pub fn draw(frame: &mut Frame, screen: Rect, app: &mut App) {
    let Some(Panel::Settings(view)) = &mut app.panel else {
        return;
    };
    view.hits.clear();
    let area = window(screen, 150);
    let title = match view.category {
        None => " settings ".to_owned(),
        Some(category) => format!(" settings / {} ", category.name()),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(NIGHT))
        .title(Line::from(Span::styled(title, bold(VIOLET))))
        .title_bottom(Line::from(Span::styled(hints(view), dim())).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    if inner.width < 10 || inner.height < 3 {
        return;
    }
    let editable = matches!(
        view.category,
        Some(SettingCategory::Agents | SettingCategory::Skills | SettingCategory::Mcp)
    );
    if view.editor.is_none() || !editable {
        return list(frame, inner, view);
    }
    let list_width = (inner.width * 3 / 10).max(24).min(inner.width / 2);
    list(
        frame,
        Rect {
            width: list_width,
            ..inner
        },
        view,
    );
    let divider = Rect::new(inner.x + list_width, inner.y, 1, inner.height);
    let line: Vec<Line> = (0..inner.height)
        .map(|_| Line::from(Span::styled("\u{2502}", fg(NIGHT))))
        .collect();
    frame.render_widget(Paragraph::new(line), divider);
    let pane = Rect {
        x: inner.x + list_width + 2,
        width: inner.width.saturating_sub(list_width + 2),
        ..inner
    };
    editor(frame, pane, view);
}

fn hints(view: &SettingsView) -> &'static str {
    match (view.category, &view.editor) {
        (None, _) => " up/down choose  enter open  esc close ",
        (Some(SettingCategory::Tools), _) => " up/down choose  space/click switch  esc back ",
        (Some(_), Some(_)) => " Ctrl+S save  Ctrl+R revert  Esc close  click another one ",
        (Some(SettingCategory::Agents), None) => {
            " up/down choose  enter edit  space switch  left/right limit  r reload  esc back "
        }
        (Some(SettingCategory::Skills), None) => {
            " up/down choose  enter open  space switch  r reload  esc back "
        }
        (Some(SettingCategory::Mcp), None) => {
            " up/down choose  enter edit  n new  c reconnect  space switch  esc back "
        }
    }
}

/// The items of the list, scrolled so the selected one shows whole, with their hits.
fn list(frame: &mut Frame, area: Rect, view: &mut SettingsView) {
    let rows = rows(view, area.width as usize);
    let height = area.height as usize;
    let shown = |from: usize, to: usize| -> usize {
        rows[from..=to.min(rows.len().saturating_sub(1))]
            .iter()
            .map(|row| row.lines.len())
            .sum()
    };
    view.top = view.top.min(view.selected);
    while view.top < view.selected && shown(view.top, view.selected) > height {
        view.top += 1;
    }
    let mut lines = Vec::new();
    for (index, row) in rows.into_iter().enumerate().skip(view.top) {
        if lines.len() >= height {
            break;
        }
        let selectable = index < view.len();
        let first = area.y + (lines.len() + row.lead) as u16;
        if let Some((from, to)) = row.switch {
            // First, so a click on the switch wins over the row under it.
            view.hits.insert(
                0,
                SettingsHit {
                    row: first,
                    left: area.x + from,
                    right: area.x + to,
                    target: HitTarget::Switch(index),
                },
            );
        }
        for (at, line) in row.lines.into_iter().enumerate() {
            if lines.len() >= height {
                break;
            }
            if at < row.lead {
                lines.push(line);
                continue;
            }
            if selectable {
                view.hits.push(SettingsHit {
                    row: area.y + lines.len() as u16,
                    left: area.x,
                    right: area.right(),
                    target: HitTarget::Row(index),
                });
            }
            lines.push(if selectable && index == view.selected {
                line.style(Style::new().bg(GLOW))
            } else {
                line
            });
        }
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn rows(view: &SettingsView, width: usize) -> Vec<Row> {
    match view.category {
        None => SettingCategory::ALL
            .iter()
            .map(|category| Row {
                lines: vec![right(
                    vec![Span::raw(format!("  {}", category.name()))],
                    Span::styled("open > ", dim()),
                    width,
                )],
                switch: None,
                lead: 0,
            })
            .collect(),
        Some(SettingCategory::Agents) => agent_rows(view, width),
        Some(SettingCategory::Tools) => tool_rows(view, width),
        Some(SettingCategory::Skills) => {
            let mut rows: Vec<Row> = view
                .skills
                .iter()
                .map(|skill| skill_row(skill, view.editor.is_none(), width))
                .collect();
            if rows.is_empty() {
                rows.push(note("  no skills: add folders with a SKILL.md to skills/"));
            }
            rows
        }
        Some(SettingCategory::Mcp) => {
            let mut rows: Vec<Row> = view
                .mcp
                .iter()
                .map(|server| mcp_row(server, width))
                .collect();
            if rows.is_empty() {
                rows.push(note("  no MCP servers: press n to add one"));
            }
            rows
        }
    }
}

/// The name, where it runs and how it stands, with the switch; then its command line or
/// URL, or why it failed.
fn mcp_row(server: &McpInfo, width: usize) -> Row {
    let on = server.state != McpState::Off;
    let (state, color) = match &server.state {
        McpState::Off => ("off".to_owned(), NIGHT),
        McpState::Starting => ("starting".to_owned(), SUN),
        McpState::Ready(tools) => (format!("{} tools", tools.len()), TEAL),
        McpState::Failed(_) => ("failed".to_owned(), EMBER),
    };
    let place = if server.remote { "remote" } else { "local" };
    let first = vec![
        Span::styled(
            format!("  {}", server.name),
            if on { bold(TEAL) } else { dim() },
        ),
        Span::raw(" "),
        Span::styled(format!(" {place} "), fg(VIOLET).bg(GLOW)),
        Span::raw(" "),
        Span::styled(format!(" {state} "), fg(color).bg(GLOW)),
    ];
    let mut row = switch_row(first, on, width);
    let (text, style) = match &server.state {
        McpState::Failed(error) => (format!("{FAILED} {error}"), fg(EMBER)),
        _ => (server.target.clone(), dim()),
    };
    row.lines.push(Line::from(vec![
        Span::raw("    "),
        Span::styled(wrap::clip(&text, width.saturating_sub(5)), style),
    ]));
    row
}

/// The name and tags with the switch, then the description, or why it cannot be used.
fn skill_row(skill: &SkillInfo, wide: bool, width: usize) -> Row {
    let mut first = vec![Span::styled(
        format!("  {}", skill.name),
        if skill.enabled { bold(TEAL) } else { dim() },
    )];
    let mut tags = vec![(skill.kind.name().to_owned(), kind_color(skill.kind))];
    tags.extend(skill.source.iter().map(|source| (source.clone(), SUN)));
    if wide {
        tags.extend(skill.tags.iter().map(|tag| (tag.clone(), VIOLET)));
    }
    for (tag, color) in tags {
        first.push(Span::raw(" "));
        first.push(Span::styled(format!(" {tag} "), fg(color).bg(GLOW)));
    }
    let mut row = switch_row(first, skill.enabled, width);
    let (text, style) = match &skill.problem {
        Some(problem) => (format!("{FAILED} {problem}"), fg(EMBER)),
        None => (skill.description.clone(), dim()),
    };
    let room = width.saturating_sub(5);
    row.lines.push(Line::from(vec![
        Span::raw("    "),
        Span::styled(wrap::clip(&text, room), style),
    ]));
    row
}

fn kind_color(kind: SkillKind) -> ratatui::style::Color {
    match kind {
        SkillKind::Builtin => VIOLET,
        SkillKind::Own => TEAL,
        SkillKind::Other => SUN,
    }
}

/// The limit first, then the tree, then what is wrong with the agents folder.
fn agent_rows(view: &SettingsView, width: usize) -> Vec<Row> {
    let mut rows = vec![Row {
        lines: vec![right(
            vec![Span::raw("  concurrent agents")],
            Span::styled(
                format!("\u{2212} {} +  ", view.max_agents_at_work),
                bold(SUN),
            ),
            width,
        )],
        switch: None,
        lead: 0,
    }];
    rows.extend(view.agents.iter().map(|agent| agent_row(agent, width)));
    for problem in &view.problems {
        rows.push(Row {
            lines: vec![Line::from(Span::styled(
                format!("  {FAILED} {}: {}", problem.node, problem.reason),
                fg(EMBER),
            ))],
            switch: None,
            lead: 0,
        });
    }
    if view.agents.len() <= 2 {
        rows.push(note(
            "  add folders with an AGENT.md to agents/ for expert agents",
        ));
    }
    rows
}

fn agent_row(agent: &AgentRow, width: usize) -> Row {
    if agent.depth == 0 {
        return Row {
            lines: vec![Line::from(vec![
                Span::styled(format!("  {ROOT} "), bold(TEAL)),
                Span::styled(agent.name.clone(), bold(TEAL)),
                Span::styled(format!("  {}", agent.description), dim()),
            ])],
            switch: None,
            lead: 0,
        };
    }
    let trace = if agent.active { TEAL } else { NIGHT };
    let mut left = vec![Span::raw("  ")];
    for &goes_on in &agent.rails {
        left.push(Span::styled(if goes_on { TRACE } else { "   " }, fg(NIGHT)));
    }
    let branch = if agent.last { LAST_BRANCH } else { BRANCH };
    left.push(Span::styled(branch, fg(trace)));
    let (pad, pad_color) = match (agent.enabled, agent.active) {
        (true, true) => (ON, TEAL),
        (true, false) => (ON, SUN),
        (false, _) => (OFF, EMBER),
    };
    left.push(Span::styled(format!("{pad} "), fg(pad_color)));
    left.push(Span::styled(
        agent.name.clone(),
        if agent.active { bold(TEAL) } else { dim() },
    ));
    if let Some(version) = agent.version {
        left.push(Span::styled(format!(" v{version}"), fg(VIOLET)));
    }
    if !agent.links.is_empty() {
        left.push(Span::styled(
            format!("  asks {}", agent.links.join(", ")),
            fg(SUN),
        ));
    }
    left.push(Span::styled(format!("  {}", agent.description), dim()));
    switch_row(left, agent.enabled, width)
}

/// `left`, clipped to leave room for an on/off switch at the right edge.
/// Tools grouped by kind, each group under its name.
fn tool_rows(view: &SettingsView, width: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut kind = None;
    for tool in &view.tools {
        let mut row = switch_row(
            vec![Span::raw(format!("    {}", tool.name))],
            tool.enabled,
            width,
        );
        if kind != Some(tool.kind) {
            kind = Some(tool.kind);
            let gap = if rows.is_empty() {
                vec![]
            } else {
                vec![Line::default()]
            };
            row.lead = gap.len() + 1;
            let heading = Line::from(Span::styled(
                format!("  {}", tool.kind.name()),
                bold(VIOLET),
            ));
            row.lines = [gap, vec![heading], row.lines].concat();
        }
        rows.push(row);
    }
    rows
}

fn switch_row(left: Vec<Span<'static>>, on: bool, width: usize) -> Row {
    let label = if on { " on  " } else { " off " };
    let switch = Span::styled(
        label,
        if on {
            bold(TEAL).bg(GLOW)
        } else {
            fg(NIGHT).bg(GLOW)
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

fn note(text: &str) -> Row {
    Row {
        lines: vec![Line::from(Span::styled(text.to_owned(), dim()))],
        switch: None,
        lead: 0,
    }
}

/// `left` with `end` pushed to the right edge of `width`.
fn right(left: Vec<Span<'static>>, end: Span<'static>, width: usize) -> Line<'static> {
    let used: usize = left.iter().map(|span| span.content.width()).sum();
    let gap = width.saturating_sub(used + end.content.width());
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(end);
    Line::from(spans)
}

fn clip_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
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

/// The open skill: its front matter as fields, then the file to edit, then the buttons.
fn editor(frame: &mut Frame, area: Rect, view: &mut SettingsView) {
    let Some(editor) = &mut view.editor else {
        return;
    };
    let width = area.width.saturating_sub(2) as usize;
    let fields = metadata(editor, width);
    let top = (fields.len() as u16 + 2)
        .min(METADATA_ROWS)
        .min(area.height / 2);
    let mut title = vec![Span::styled(format!(" {} ", editor.title()), bold(TEAL))];
    if editor.changed() {
        title.push(Span::styled("changed ", fg(SUN)));
    }
    let header = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(NIGHT))
        .title(Line::from(title));
    frame.render_widget(
        Paragraph::new(fields).block(header),
        Rect {
            height: top,
            ..area
        },
    );
    if area.height <= top + 2 {
        return;
    }
    let text_area = Rect {
        y: area.y + top,
        height: area.height - top - 1,
        ..area
    };
    let (rows, (column, row)) = input::layout(&editor.text, text_area.width as usize);
    let height = text_area.height as usize;
    if row < editor.scroll {
        editor.scroll = row;
    } else if row >= editor.scroll + height {
        editor.scroll = row + 1 - height;
    }
    let lines: Vec<Line> = rows
        .into_iter()
        .skip(editor.scroll)
        .take(height)
        .map(|row| Line::from(row.spans))
        .collect();
    frame.render_widget(Paragraph::new(lines), text_area);
    let y = text_area.y + (row - editor.scroll) as u16;
    frame.set_cursor_position(Position::new(text_area.x + column as u16, y));
    let bar = area.bottom() - 1;
    if let Some(error) = &editor.error {
        let room = area.width.saturating_sub(46) as usize;
        let line = Line::from(Span::styled(wrap::clip(error, room), fg(EMBER)));
        frame.render_widget(Paragraph::new(line), Rect::new(area.x, bar, area.width, 1));
    }
    let buttons = [
        (FileButton::Save, " save ", TEAL),
        (FileButton::Revert, " revert ", SUN),
        (FileButton::Close, " close ", ROSE),
    ];
    let mut right = area.right();
    for &(button, label, color) in buttons.iter().rev() {
        let width = label.width() as u16;
        let Some(left) = right.checked_sub(width + 1).filter(|&left| left >= area.x) else {
            break;
        };
        let shown = Line::from(Span::styled(label, bold(color).bg(GLOW)));
        frame.render_widget(Paragraph::new(shown), Rect::new(left, bar, width, 1));
        view.hits.insert(
            0,
            SettingsHit {
                row: bar,
                left,
                right: left + width,
                target: HitTarget::Button(button),
            },
        );
        right = left;
    }
}

/// The description in full, then every other plain field on a line of its own.
fn metadata(editor: &FileEditor, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some((_, description)) = editor.metadata.iter().find(|(key, _)| key == "description") {
        lines.extend(
            wrap::words(vec![Span::raw(description.clone())], width, 0)
                .into_iter()
                .map(Line::from),
        );
    }
    for (key, value) in &editor.metadata {
        if key == "description" || key == "name" {
            continue;
        }
        lines.push(Line::from(vec![
            Span::styled(format!("{key} {DOT} "), fg(VIOLET)),
            Span::styled(
                wrap::clip(value, width.saturating_sub(key.len() + 3)),
                dim(),
            ),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "no front matter: the file must start with --- and a description",
            fg(EMBER),
        )));
    }
    lines
}
