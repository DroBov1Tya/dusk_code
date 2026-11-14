//! The typed text laid out in rows.

use super::{PROMPT, SIDE};
use crate::app::{App, Focus, Input, paste};
use crate::ui::thread::{
    theme::{RAISED, SELECTED, TEXT_SOFT, fg},
    wrap,
};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::Style,
    text::Span,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Columns before the own answer: the pointer and the pencil.
pub(super) const OWN_LEAD: usize = 6;

/// A longer answer shows its last lines, where the cursor usually is.
pub(super) const MAX_OWN_LINES: usize = 5;

/// The own answer wrapped to the box, at most `MAX_OWN_LINES` of it.
pub(super) fn own_lines(text: &str, width: usize) -> Vec<String> {
    let lines = wrap::chars(text, width.saturating_sub(OWN_LEAD));
    let skip = lines.len().saturating_sub(MAX_OWN_LINES);
    lines.into_iter().skip(skip).collect()
}

pub(super) fn show_cursor(frame: &mut Frame, app: &App, area: Rect, (column, row): (usize, usize)) {
    let polling = app.poll.as_ref().is_some_and(|poll| poll.active);
    if app.panel.is_none() && app.focus == Focus::Input && !polling && app.screensaver.is_none() {
        let x = area.x + PROMPT.width() as u16 + column as u16;
        frame.set_cursor_position(Position::new(x, area.y + row as u16));
    }
}

pub(super) fn text_width(width: u16) -> usize {
    (width as usize)
        .saturating_sub(PROMPT.width() + 1 + 2 * SIDE as usize)
        .max(1)
}

/// A row of the box: its spans, where its chips are as (from, to, paste index), and where
/// each character starts as (column, byte offset), up to the offset that ends the row.
#[derive(Default)]
pub struct Row {
    pub spans: Vec<Span<'static>>,
    pub(super) chips: Vec<(usize, usize, usize)>,
    pub(super) cells: Vec<(usize, usize)>,
    pub(super) end: usize,
    pub(super) plain: String,
    pub(super) plain_selected: bool,
    pub(super) width: usize,
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
                let style = fg(TEXT_SOFT).bg(if selected { SELECTED } else { RAISED });
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
