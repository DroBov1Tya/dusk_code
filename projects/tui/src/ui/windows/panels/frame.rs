//! The panel on screen: which one draws, the window it sits in, and the rows they share.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::connection::connection;
use super::context::context;
use super::diff;
use super::help::help;
use super::jobs::jobs;
use super::models::models;
use super::paste::draw_paste;
use super::plugins::plugins;
use super::threads::draw_threads;
use super::unsent::unsent;
use super::update::draw_update;
use crate::app::{App, EFFORTS, Panel, compact_number, paste};
use crate::ui::windows::{
    agent_window, settings,
    theme::{DOT, DUSK, HAIRLINE, ON, RAISED, TEXT_MUTED, bold, dim, fg},
    wrap,
};

pub fn draw(frame: &mut Frame, screen: Rect, app: &mut App) {
    app.editor_hits.clear();
    if let Some(Panel::Connect(form)) = &mut app.panel {
        form.field_areas.fill(None);
    }
    match app.panel {
        Some(Panel::Paste(_)) => return draw_paste(frame, screen, app),
        Some(Panel::Update { .. }) => return draw_update(frame, screen, app),
        Some(Panel::Agent(_)) => return agent_window::draw(frame, screen, app),
        Some(Panel::Settings(_)) => return settings::draw(frame, screen, app),
        _ => {}
    }
    let Some(panel) = &app.panel else {
        return;
    };
    let mut connection_rows = Vec::new();
    let (title, hints, lines, selected) = match panel {
        Panel::Threads(list) => return draw_threads(frame, screen, app, list),
        Panel::Settings(_) => return,
        Panel::Connect(form) => {
            let (lines, selected, rows) = connection(form, screen.width.saturating_sub(8) as usize);
            connection_rows = rows;
            let hints = match form.preset {
                None => "up/down choose  enter pick  esc cancel",
                Some(preset) if preset.needs_key() => {
                    "up/down/tab focus  enter next/save  ctrl+o key page  esc back"
                }
                Some(_) => "up/down/tab focus  enter next/save  esc back",
            };
            ("connect provider", hints, lines, Some(selected))
        }
        Panel::Plugins { selected, entry } => {
            let (lines, row) = plugins(
                app,
                *selected,
                entry.as_ref(),
                screen.width.saturating_sub(8) as usize,
            );
            (
                "plugins",
                if entry.is_some() {
                    "enter keep for this session  esc cancel"
                } else {
                    "up/down choose  enter switch on/off  s enter a secret  esc close"
                },
                lines,
                Some(row),
            )
        }
        Panel::Unsent { selected } => {
            let (lines, row) = unsent(app, *selected, screen.width.saturating_sub(8) as usize);
            (
                "local unsent messages",
                "up/down choose  enter send selected  esc close",
                lines,
                Some(row),
            )
        }
        Panel::Help => ("keys", "any key closes", help(), None),
        Panel::Effort { selected } => {
            let current = app.effort.map_or("default", |effort| effort.name());
            let lines = EFFORTS
                .iter()
                .enumerate()
                .map(|(index, effort)| {
                    let mark = if *effort == current { "current" } else { "" };
                    selectable(
                        vec![Span::raw(format!(" {effort}"))],
                        Span::styled(format!("{mark} "), dim()),
                        index == *selected,
                    )
                })
                .collect();
            (
                "reasoning effort",
                "enter choose  esc close",
                lines,
                Some(*selected),
            )
        }
        Panel::Undo { points, selected } => {
            let room = screen.width.saturating_sub(16).min(66) as usize;
            let lines = points
                .iter()
                .enumerate()
                .map(|(index, point)| {
                    let text = paste::labelled(&point.text).replace('\n', " ");
                    selectable(
                        vec![
                            Span::styled(format!(" {:>3}  ", index + 1), dim()),
                            Span::raw(wrap::clip(&text, room)),
                        ],
                        Span::raw(" "),
                        index == *selected,
                    )
                })
                .collect();
            (
                "undo: go back to before",
                "up/down choose  enter go back, the text returns to the box  esc close",
                lines,
                Some(*selected),
            )
        }
        Panel::Sources {
            sources,
            selected,
            removing,
        } => {
            let room = screen.width.saturating_sub(34).min(44) as usize;
            let lines = sources
                .iter()
                .enumerate()
                .map(|(index, source)| {
                    let (mark, when) = match (source.pinned, source.delivered) {
                        (true, _) => (ON, "every message"),
                        (false, false) => (DOT, "next message"),
                        (false, true) => (DOT, "kept"),
                    };
                    selectable(
                        vec![
                            Span::styled(format!(" {mark} "), fg(TEXT_MUTED)),
                            Span::raw(wrap::clip(&source.name, room)),
                        ],
                        Span::styled(
                            format!("{} {DOT} {when} ", compact_number(source.bytes)),
                            dim(),
                        ),
                        index == *selected,
                    )
                })
                .collect();
            (
                "sources of this chat",
                if *removing {
                    "d again deletes the file  any other key keeps it"
                } else {
                    "space every message or once  d delete  esc close"
                },
                lines,
                Some(*selected),
            )
        }
        Panel::Jobs(view) => {
            let width = screen.width.saturating_sub(8).min(72) as usize;
            (
                "background jobs",
                "up/down choose  x stop  r read again  esc close",
                jobs(view, width),
                Some(view.selected),
            )
        }
        Panel::Context { report, .. } => (
            "context",
            "up/down scroll  any other key closes",
            context(report),
            None,
        ),
        Panel::Diff(view) => {
            let width = screen.width.saturating_sub(8).min(116) as usize;
            let (lines, hints) = match view.open {
                true => (
                    diff::file(view, width),
                    "up/down pgup/pgdn scroll  esc back to the files",
                ),
                false => (
                    diff::list(view, width),
                    "up/down choose  enter open  esc close",
                ),
            };
            (
                "changes",
                hints,
                lines,
                (!view.open).then_some(view.selected),
            )
        }
        Panel::Models(list) => {
            let (lines, row) = models(list, screen.width.saturating_sub(8).min(72) as usize);
            (
                "model",
                "enter choose  r check again  esc close",
                lines,
                Some(row),
            )
        }
        Panel::Paste(_) | Panel::Agent(_) | Panel::Update { .. } => return,
    };
    let cap = if matches!(app.panel, Some(Panel::Diff(_))) {
        120
    } else {
        76
    };
    let width = screen.width.saturating_sub(4).min(cap);
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
    let skip = match &app.panel {
        // The context scrolls by its top row rather than following a selection.
        Some(Panel::Context { row, .. }) => (*row).min(lines.len().saturating_sub(inner)),
        Some(Panel::Diff(view)) if view.open => view.scroll.min(lines.len().saturating_sub(inner)),
        _ => selected.map_or(0, |row| (row + 1).saturating_sub(inner)),
    };
    match &mut app.panel {
        Some(Panel::Context { row, .. }) => *row = skip,
        Some(Panel::Diff(view)) if view.open => view.scroll = skip,
        _ => {}
    }
    if let Some(Panel::Connect(form)) = &mut app.panel {
        for (index, row) in connection_rows.into_iter().enumerate() {
            let start = row.max(skip);
            let end = (row + 2).min(skip + inner);
            if start < end {
                form.field_areas[index] = Some(Rect::new(
                    area.x + 1,
                    area.y + 1 + (start - skip) as u16,
                    area.width.saturating_sub(2),
                    (end - start) as u16,
                ));
            }
        }
    }
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(HAIRLINE))
        .title(Line::from(Span::styled(format!(" {title} "), bold(DUSK))))
        .title_bottom(Line::from(Span::styled(format!(" {hints} "), dim())).right_aligned());
    let lines = fit(lines, width.saturating_sub(2) as usize);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(block).scroll((skip as u16, 0)),
        area,
    );
}

/// A window centred on the screen, at most `max_width` wide, with a margin around it.
pub fn window(screen: Rect, max_width: u16) -> Rect {
    let width = screen.width.saturating_sub(6).min(max_width);
    let height = screen.height.saturating_sub(4).max(3);
    Rect {
        x: screen.x + (screen.width - width) / 2,
        y: screen.y + (screen.height - height) / 2,
        width,
        height,
    }
}

/// A row with something on the right, highlighted when selected. The right part is
/// placed by `fit` once the width is known.
pub fn selectable(left: Vec<Span<'static>>, right: Span<'static>, selected: bool) -> Line<'static> {
    let mut spans = left;
    spans.push(Span::raw("\t"));
    spans.push(right);
    let line = Line::from(spans);
    if selected {
        line.style(Style::new().bg(RAISED).add_modifier(Modifier::BOLD))
    } else {
        line
    }
}

/// Pushes the part after a tab to the right edge, clipping what does not fit.
pub fn fit(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
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

pub fn ago(ms: i64) -> String {
    match age(ms).as_str() {
        "now" => "just now".into(),
        age => format!("{age} ago"),
    }
}

pub fn age(ms: i64) -> String {
    let minutes = ms.max(0) / 60_000;
    match minutes {
        0 => "now".into(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}
