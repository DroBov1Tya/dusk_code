//! The bottom of the screen: a rule that tells what the agent is doing and how full the
//! context is, then the message being typed. Typing `/` lists the commands.

use dusk_core::commands::COMMANDS;
use ratatui::{
    Frame,
    layout::{Position, Rect},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    theme::{EMBER, HAZE, HORIZON_DARK, NIGHT, ROSE, SUN, TEAL, VIOLET, bold, dim, fg, thought},
    transcript::spinner,
};
use crate::app::{Activity, App, Focus};

const MAX_ROWS: usize = 8;
const PROMPT: &str = " \u{203A} ";

/// Rows the box needs: the rule and the wrapped text.
pub fn height(app: &App, width: u16) -> u16 {
    let (rows, _) = layout(&app.input.text, app.input.cursor, text_width(width));
    rows.len().clamp(1, MAX_ROWS) as u16 + 1
}

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let rule = Rect { height: 1, ..area };
    frame.render_widget(Paragraph::new(status(app, area.width as usize)), rule);
    let text_area = Rect {
        y: area.y + 1,
        height: area.height - 1,
        ..area
    };
    let width = text_width(area.width);
    let prompt = Span::styled(PROMPT, bold(ROSE));
    if app.input.text.is_empty() {
        let hint = if app.is_busy() {
            "Enter queues for the next turn, Alt+Enter interrupts, Esc stops"
        } else {
            "Message, or /compact. Alt+Enter interrupts, Ctrl+J new line, F1 keys"
        };
        let line = Line::from(vec![prompt, Span::styled(hint, thought())]);
        frame.render_widget(Paragraph::new(line), text_area);
        show_cursor(frame, app, text_area, (0, 0));
        return;
    }
    let (rows, cursor) = layout(&app.input.text, app.input.cursor, width);
    let skip = (cursor.1 + 1).saturating_sub(text_area.height as usize);
    let lines: Vec<Line> = rows
        .into_iter()
        .enumerate()
        .skip(skip)
        .map(|(row, text)| {
            let lead = if row == 0 {
                prompt.clone()
            } else {
                Span::raw("   ")
            };
            Line::from(vec![lead, Span::raw(text)])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), text_area);
    show_cursor(frame, app, text_area, (cursor.0, cursor.1 - skip));
}

/// The command list above the box while a command is being typed.
pub fn draw_commands(frame: &mut Frame, above: Rect, app: &mut App) {
    app.command_popup = None;
    let text = app.input.text.as_str();
    if !text.starts_with('/') || text.contains(char::is_whitespace) {
        return;
    }
    let matching: Vec<_> = COMMANDS
        .iter()
        .filter(|(name, _)| name.starts_with(text))
        .collect();
    if matching.is_empty() {
        return;
    }
    let width = matching
        .iter()
        .map(|(name, about)| name.width() + about.width() + 6)
        .max()
        .unwrap_or(0)
        .min(above.width as usize) as u16;
    let height = matching.len() as u16 + 2;
    let area = Rect {
        x: above.x + 1,
        y: above.bottom().saturating_sub(height),
        width,
        height: height.min(above.height),
    };
    app.command_popup = Some(area);
    let lines: Vec<Line> = matching
        .iter()
        .map(|(name, about)| {
            Line::from(vec![
                Span::styled(format!(" {name}"), bold(VIOLET)),
                Span::styled(format!("  {about}"), dim()),
            ])
        })
        .collect();
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(NIGHT));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// The rule: what the agent is doing and how far the transcript is scrolled.
fn status(app: &App, width: usize) -> Line<'static> {
    let doing = match (&app.activity, app.focus) {
        (Activity::Idle, Focus::Browse(_)) => {
            vec![Span::styled("up/down move  enter fold  esc back", dim())]
        }
        (Activity::Idle, Focus::Input) => vec![Span::styled("ready  /  F2 sidebar", dim())],
        (activity, _) => {
            let (color, what) = match activity {
                Activity::Thinking => (SUN, "thinking".to_owned()),
                Activity::Writing => (SUN, "writing".to_owned()),
                Activity::Tool(name) => (TEAL, format!("running {name}")),
                Activity::Compacting => (VIOLET, "compacting the context".to_owned()),
                Activity::Retrying(attempt) => (EMBER, format!("retrying after attempt {attempt}")),
                Activity::Idle => (HAZE, String::new()),
            };
            vec![
                Span::styled(format!("{} ", spinner(app)), fg(color)),
                Span::styled(what, fg(color)),
            ]
        }
    };
    let below = match (&app.flash, app.scroll) {
        (Some(flash), _) => flash.clone(),
        (None, 0) => String::new(),
        (None, scroll) => format!("{scroll} lines below"),
    };
    let left_width: usize = doing.iter().map(Span::width).sum::<usize>() + 4;
    let right_width = below.width() + 2;
    let fill_width = width.saturating_sub(left_width + right_width);
    let mut spans = vec![Span::styled(
        format!("{HORIZON_DARK}{HORIZON_DARK} "),
        fg(NIGHT),
    )];
    spans.extend(doing);
    spans.push(Span::styled(
        format!(" {} ", HORIZON_DARK.repeat(fill_width)),
        fg(NIGHT),
    ));
    if !below.is_empty() {
        spans.push(Span::styled(format!(" {below} "), fg(SUN)));
    }
    Line::from(spans)
}

fn show_cursor(frame: &mut Frame, app: &App, area: Rect, (column, row): (usize, usize)) {
    if app.panel.is_none() && app.focus == Focus::Input {
        let x = area.x + PROMPT.width() as u16 + column as u16;
        frame.set_cursor_position(Position::new(x, area.y + row as u16));
    }
}

fn text_width(width: u16) -> usize {
    (width as usize).saturating_sub(PROMPT.width() + 1).max(1)
}

/// The text broken into rows of `width` columns, and the cursor as (column, row).
fn layout(text: &str, cursor: usize, width: usize) -> (Vec<String>, (usize, usize)) {
    let mut rows = vec![String::new()];
    let mut column = 0;
    let mut at = (0, 0);
    for (offset, c) in text.char_indices() {
        if offset == cursor {
            at = (column, rows.len() - 1);
        }
        if c == '\n' {
            rows.push(String::new());
            column = 0;
            continue;
        }
        let char_width = c.width().unwrap_or(0);
        if column + char_width > width {
            rows.push(String::new());
            column = 0;
        }
        if let Some(row) = rows.last_mut() {
            row.push(c);
        }
        column += char_width;
    }
    if cursor >= text.len() {
        at = (column, rows.len() - 1);
    }
    (rows, at)
}
