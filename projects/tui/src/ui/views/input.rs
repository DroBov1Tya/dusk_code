//! The bottom of the screen: a rule that tells what the agent is doing and how full the
//! context is, then the message being typed, on a background of its own. Typing `/`
//! lists the commands.

use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{
    theme::{
        EMBER, GLOW, HAZE, HORIZON_DARK, INPUT_BG, NIGHT, ROSE, SELECTED, SUN, TEAL, VIOLET, bold,
        dim, fg, thought,
    },
    transcript::spinner,
    wrap,
};
use crate::app::{
    Activity, App, ChipHit, Focus, Input, InputRow, PollHit, PollTarget, QueueButton, QueueHit,
    commands, paste,
};
use crate::git::GitSummary;

const MIN_ROWS: usize = 3;
const MAX_ROWS: usize = 10;
/// Queued messages shown above the box; older ones wait unseen.
const MAX_QUEUED: usize = 4;
const PROMPT: &str = " \u{203A} ";
/// Rows around the text: the rule above, and a blank row on each side of the text.
const FRAME_ROWS: u16 = 3;

/// Rows the box needs: the queued messages, the rule, the padding and the wrapped text.
pub fn height(app: &App, width: u16) -> u16 {
    let (rows, _) = layout(&app.input, text_width(width));
    poll_rows(app)
        + queue_rows(app)
        + rows.len().clamp(MIN_ROWS, MAX_ROWS) as u16
        + FRAME_ROWS
        + u16::from(app.git.is_some())
}

/// The question's border, its text, the options, the own answer and the buttons.
fn poll_rows(app: &App) -> u16 {
    app.poll
        .as_ref()
        .map_or(0, |poll| poll.question.options.len() as u16 + 5)
}

fn queue_rows(app: &App) -> u16 {
    app.queued.len().min(MAX_QUEUED) as u16
}

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let polled = poll_rows(app);
    draw_poll(
        frame,
        Rect {
            height: polled,
            ..area
        },
        app,
    );
    let area = Rect {
        y: area.y + polled,
        height: area.height.saturating_sub(polled),
        ..area
    };
    if let Some(summary) = &app.git {
        let bottom = Rect {
            y: area.bottom().saturating_sub(1),
            height: 1,
            ..area
        };
        frame.render_widget(Paragraph::new(git_line(summary)), bottom);
    }
    let area = Rect {
        height: area.height.saturating_sub(u16::from(app.git.is_some())),
        ..area
    };
    let queued = queue_rows(app);
    draw_queue(
        frame,
        Rect {
            height: queued,
            ..area
        },
        app,
    );
    let area = Rect {
        y: area.y + queued,
        height: area.height.saturating_sub(queued),
        ..area
    };
    let rule = Rect { height: 1, ..area };
    frame.render_widget(Paragraph::new(status(app, area.width as usize)), rule);
    let box_area = Rect {
        y: area.y + 1,
        height: area.height.saturating_sub(1),
        ..area
    };
    frame.render_widget(Block::new().style(Style::new().bg(INPUT_BG)), box_area);
    let text_area = Rect {
        y: box_area.y + 1,
        height: box_area.height.saturating_sub(2),
        ..box_area
    };
    app.input_chips.clear();
    app.input_rows.clear();
    app.input_area = Some(box_area);
    let width = text_width(area.width);
    let prompt = Span::styled(PROMPT, bold(ROSE));
    if app.input.text.is_empty() {
        let hint = if !app.provider_available {
            "provider unavailable; /connect or /model to choose a provider"
        } else if app.is_busy() {
            "write to queue it; Alt+Enter sends now, Esc stops"
        } else {
            "write, or / for commands"
        };
        let line = Line::from(vec![prompt, Span::styled(hint, thought())]);
        frame.render_widget(Paragraph::new(line), text_area);
        show_cursor(frame, app, text_area, (0, 0));
        return;
    }
    let (rows, cursor) = layout(&app.input, width);
    let skip = (cursor.1 + 1).saturating_sub(text_area.height as usize);
    let left = text_area.x + PROMPT.width() as u16;
    let mut lines = Vec::new();
    for (row, shown) in rows.into_iter().enumerate().skip(skip) {
        let y = text_area.y + (row - skip) as u16;
        app.input_rows.push(InputRow {
            row: y,
            right: left + shown.width as u16,
            cells: shown
                .cells
                .iter()
                .map(|&(column, offset)| (left + column as u16, offset))
                .collect(),
            end: shown.end,
        });
        for &(from, to, index) in &shown.chips {
            app.input_chips.push(ChipHit {
                row: y,
                left: left + from as u16,
                right: left + to as u16,
                index,
            });
        }
        let lead = if row == 0 {
            prompt.clone()
        } else {
            Span::raw("   ")
        };
        lines.push(Line::from([vec![lead], shown.spans].concat()));
    }
    frame.render_widget(Paragraph::new(lines), text_area);
    show_cursor(frame, app, text_area, (cursor.0, cursor.1 - skip));
}

/// The repository, branch, commits, changed files and the distance to the upstream.
fn git_line(summary: &GitSummary) -> Line<'static> {
    let dot = || Span::styled(" \u{00B7} ", dim());
    let mut spans = vec![
        Span::styled(format!("  {}", summary.repository), dim()),
        dot(),
        Span::styled(
            summary.branch.clone().unwrap_or_else(|| "detached".into()),
            fg(TEAL),
        ),
        dot(),
        Span::styled(format!("{} commits", summary.commits), dim()),
        dot(),
    ];
    spans.push(match summary.changed {
        0 => Span::styled("clean", dim()),
        changed => Span::styled(format!("{changed} changed"), fg(SUN)),
    });
    if let Some((ahead, behind)) = summary.ahead_behind {
        spans.push(dot());
        spans.push(Span::styled(
            format!("\u{2191}{ahead} \u{2193}{behind}"),
            if ahead + behind == 0 {
                dim()
            } else {
                fg(VIOLET)
            },
        ));
    }
    Line::from(spans)
}

const SEND: &str = " send ";
const DECLINE: &str = " decline ";

/// The question the agent asked: options to choose, the user's own answer, and buttons.
fn draw_poll(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(poll) = &mut app.poll else {
        return;
    };
    poll.hits.clear();
    if area.height < 4 || area.width < 20 {
        return;
    }
    let border = if poll.active { VIOLET } else { NIGHT };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(border))
        .title(Span::styled(" question ", bold(VIOLET)));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let width = inner.width as usize;
    let mut lines = vec![Line::from(Span::styled(
        wrap::clip(&format!(" {}", poll.question.question), width),
        bold(SUN),
    ))];
    let own_row = poll.own_row();
    for row in 0..=own_row {
        let here = poll.active && poll.cursor == row;
        let pointer = if here { " \u{203A} " } else { "   " };
        let (mark, text) = if row < own_row {
            let mark = match (poll.question.multiple, poll.checked[row]) {
                (true, true) => "[x] ",
                (true, false) => "[ ] ",
                (false, _) if here => "(\u{2022}) ",
                (false, _) => "( ) ",
            };
            (mark, poll.question.options[row].clone())
        } else {
            let own = if poll.own.text.is_empty() && !here {
                "your own answer".to_owned()
            } else {
                poll.own.text.clone()
            };
            ("\u{270E}  ", own)
        };
        let style = if here { bold(ROSE) } else { Style::new() };
        let label = wrap::clip(&format!("{pointer}{mark}{text}"), width);
        poll.hits.push(PollHit {
            row: inner.y + 1 + row as u16,
            left: inner.x,
            right: inner.right(),
            target: PollTarget::Row(row),
        });
        lines.push(Line::from(Span::styled(label, style)));
    }
    let hint = if poll.question.multiple {
        "space marks  enter sends  tab to the box  esc declines"
    } else {
        "enter sends  tab to the box  esc declines"
    };
    let buttons_row = inner.y + lines.len() as u16;
    let send_left = inner.x + 1;
    let decline_left = send_left + SEND.width() as u16 + 1;
    poll.hits.push(PollHit {
        row: buttons_row,
        left: send_left,
        right: send_left + SEND.width() as u16,
        target: PollTarget::Send,
    });
    poll.hits.push(PollHit {
        row: buttons_row,
        left: decline_left,
        right: decline_left + DECLINE.width() as u16,
        target: PollTarget::Decline,
    });
    lines.push(Line::from(vec![
        Span::raw(" "),
        Span::styled(SEND, fg(TEAL).bg(GLOW)),
        Span::raw(" "),
        Span::styled(DECLINE, fg(EMBER).bg(GLOW)),
        Span::styled(format!("  {hint}"), dim()),
    ]));
    frame.render_widget(Paragraph::new(lines), inner);
    if poll.active && poll.cursor == own_row {
        let column = 6 + poll.own.text[..poll.own.cursor].width();
        let x = (inner.x as usize + column).min(inner.right().saturating_sub(1) as usize);
        frame.set_cursor_position(Position::new(x as u16, inner.y + 1 + own_row as u16));
    }
}

/// The messages waiting for the turn to end, each with its buttons; the newest are shown
/// when there are more than fit.
fn draw_queue(frame: &mut Frame, area: Rect, app: &mut App) {
    app.queue_hits.clear();
    let skip = app.queued.len().saturating_sub(area.height as usize);
    const SEND: &str = " send now ";
    const REMOVE: &str = " \u{00D7} ";
    let buttons = SEND.width() + REMOVE.width() + 1;
    let mut lines = Vec::new();
    for (index, text) in app.queued.iter().enumerate().skip(skip) {
        let row = area.y + lines.len() as u16;
        let label = format!("   \u{29D7} {}", paste::labelled(text).replace('\n', " "));
        let room = (area.width as usize).saturating_sub(buttons);
        let label = wrap::clip(&label, room);
        let gap = room.saturating_sub(label.width());
        let send_left = area.x + (label.width() + gap) as u16;
        let remove_left = send_left + SEND.width() as u16 + 1;
        let hits = [
            (area.x, send_left, QueueButton::Edit),
            (
                send_left,
                send_left + SEND.width() as u16,
                QueueButton::SendNow,
            ),
            (
                remove_left,
                remove_left + REMOVE.width() as u16,
                QueueButton::Remove,
            ),
        ];
        for (left, right, button) in hits {
            app.queue_hits.push(QueueHit {
                row,
                left,
                right,
                index,
                button,
            });
        }
        lines.push(Line::from(vec![
            Span::styled(label, thought()),
            Span::raw(" ".repeat(gap)),
            Span::styled(SEND, fg(SUN).bg(GLOW)),
            Span::raw(" "),
            Span::styled(REMOVE, fg(EMBER).bg(GLOW)),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// The command list above the box while a command is being typed.
pub fn draw_commands(frame: &mut Frame, above: Rect, app: &mut App) {
    app.command_popup = None;
    let text = app.input.text.as_str();
    if !text.starts_with('/') || text.contains(char::is_whitespace) {
        return;
    }
    let matching: Vec<_> = commands()
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
        (Activity::Idle, Focus::Input) => {
            let state = if app.provider_available {
                "ready"
            } else {
                "provider unavailable"
            };
            let saved = if app.local_unsent.is_empty() {
                String::new()
            } else {
                format!("  {} unsent \u{00B7} /resend", app.local_unsent.len())
            };
            vec![Span::styled(
                format!("{state}{saved}  /  F2 sidebar"),
                dim(),
            )]
        }
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
    let polling = app.poll.as_ref().is_some_and(|poll| poll.active);
    if app.panel.is_none() && app.focus == Focus::Input && !polling {
        let x = area.x + PROMPT.width() as u16 + column as u16;
        frame.set_cursor_position(Position::new(x, area.y + row as u16));
    }
}

fn text_width(width: u16) -> usize {
    (width as usize).saturating_sub(PROMPT.width() + 1).max(1)
}

/// A row of the box: its spans, where its chips are as (from, to, paste index), and where
/// each character starts as (column, byte offset), up to the offset that ends the row.
#[derive(Default)]
pub struct Row {
    pub spans: Vec<Span<'static>>,
    chips: Vec<(usize, usize, usize)>,
    cells: Vec<(usize, usize)>,
    end: usize,
    plain: String,
    plain_selected: bool,
    width: usize,
}

impl Row {
    fn flush(&mut self) {
        if !self.plain.is_empty() {
            let text = std::mem::take(&mut self.plain);
            self.spans.push(match self.plain_selected {
                true => Span::styled(text, Style::new().bg(SELECTED)),
                false => Span::raw(text),
            });
        }
    }

    fn push_plain(&mut self, c: char, selected: bool) {
        if selected != self.plain_selected {
            self.flush();
            self.plain_selected = selected;
        }
        self.plain.push(c);
    }
}

/// The text broken into rows of `width` columns, chips drawn as their labels, the
/// selection marked, and the cursor as (column, row).
pub fn layout(input: &Input, width: usize) -> (Vec<Row>, (usize, usize)) {
    let selection = input.selection();
    let mut rows = vec![Row::default()];
    let mut at = (0, 0);
    for (offset, c) in input.text.char_indices() {
        if offset == input.cursor {
            at = (rows.last().map_or(0, |row| row.width), rows.len() - 1);
        }
        let selected = selection.is_some_and(|(from, to)| offset >= from && offset < to);
        if c == '\n' {
            if let Some(row) = rows.last_mut() {
                row.end = offset;
            }
            rows.push(Row::default());
            continue;
        }
        let chip = paste::chip_index(c).and_then(|index| {
            let pasted = input.pastes.get(index)?;
            Some((
                index,
                wrap::clip(&format!("[{}]", paste::label(pasted)), width),
            ))
        });
        let char_width = match &chip {
            Some((_, label)) => label.width(),
            None => c.width().unwrap_or(0),
        };
        if rows
            .last()
            .is_some_and(|row| row.width + char_width > width)
        {
            if let Some(row) = rows.last_mut() {
                row.end = offset;
            }
            rows.push(Row::default());
        }
        let Some(row) = rows.last_mut() else {
            continue;
        };
        row.cells.push((row.width, offset));
        match chip {
            Some((index, label)) => {
                row.flush();
                row.chips.push((row.width, row.width + char_width, index));
                let style = fg(TEAL).bg(if selected { SELECTED } else { GLOW });
                row.spans.push(Span::styled(label, style));
            }
            None => row.push_plain(c, selected),
        }
        row.width += char_width;
    }
    if input.cursor >= input.text.len() {
        at = (rows.last().map_or(0, |row| row.width), rows.len() - 1);
    }
    if let Some(row) = rows.last_mut() {
        row.end = input.text.len();
    }
    for row in &mut rows {
        row.flush();
    }
    (rows, at)
}
