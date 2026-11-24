//! The thread on screen. Text blocks get room around them; thinking, tool calls and
//! summaries fold into one line each and stack tightly, so the conversation stays in view.

mod blocks;
mod lines;
mod shown;
mod suggested;

pub use lines::thread_lines;
use lines::{Placed, append_entry, ends_turn, is_meta};

use super::theme::{
    ACCENT, DUSK, HAIRLINE, RAISED, SELECTED, SPINNER, SUN_GLYPH, TEXT_MUTED, TEXT_SOFT, bold, dim,
    fg,
};
use crate::app::{
    Activity, App, Button, ButtonHit, Entry, Focus, ImageSpot, Layout, ShownPreview, TEXT_COLUMN,
    TranscriptHit,
};
use ratatui::{
    Frame,
    layout::{Alignment, Rect, Size},
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

/// Scrolled up no further than this, the view follows new output to the bottom.
const FOLLOW_ROWS: usize = 3;

/// The thread's transcript: its entries, the buttons under them and the scroll position.
pub struct TranscriptView<'a> {
    pub app: &'a mut App,
}

/// What the lines of the entry at `index` are made from: when it changes, the entry is
/// laid out again. Its own stamp, and what its neighbours decide of how it is drawn.
fn signature(transcript: &crate::app::Transcript, index: usize, browsed: Option<usize>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let entries = &transcript.entries;
    transcript.stamp(index).hash(&mut hash);
    transcript.is_hidden(index).hash(&mut hash);
    transcript.is_folded_away(index).hash(&mut hash);
    (browsed == Some(index)).hash(&mut hash);
    (index > 0 && is_meta(&entries[index - 1]) && is_meta(&entries[index])).hash(&mut hash);
    ends_turn(entries, index).hash(&mut hash);
    // A running card's clock moves once a second.
    if let Entry::Run { card, .. } = &entries[index]
        && card.running
    {
        (dusk_protocol::system::now_ms() / 1000).hash(&mut hash);
    }
    if matches!(entries[index], Entry::Activity { .. }) {
        // The activity line counts its run and names what goes on while it is the newest.
        transcript.current_step(index).hash(&mut hash);
        for step in index + 1..index + 1 + transcript.run(index).count() {
            transcript.stamp(step).hash(&mut hash);
        }
    }
    hash.finish()
}

fn plain_text(line: &Line) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// `at` as the local time of day, with the date when it was not today.
fn clock(at: i64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let Some(time) = Local.timestamp_millis_opt(at).single() else {
        return String::new();
    };
    let now = Local::now();
    if time.date_naive() == now.date_naive() {
        time.format("%H:%M").to_string()
    } else if time.year() == now.year() {
        time.format("%-d %b %H:%M").to_string()
    } else {
        time.format("%-d %b %Y %H:%M").to_string()
    }
}

pub fn spinner(app: &App) -> &'static str {
    SPINNER[(app.frame / 3) % SPINNER.len()]
}

/// A new thread: the sun on the horizon and what to do.
fn welcome(frame: &mut Frame, area: Rect) {
    let width = (area.width as usize).min(44);
    // The last light sits on the horizon under the sun, the violet sky beside it.
    let horizon: Vec<Span> = (0..width)
        .map(|x| {
            let off_center = (x as f64 + 0.5 - width as f64 / 2.0).abs() / (width as f64 / 2.0);
            let color = match off_center {
                d if d < 0.12 => ACCENT,
                d if d < 0.4 => DUSK,
                _ => HAIRLINE,
            };
            Span::styled("\u{2500}", fg(color))
        })
        .collect();
    let lines = vec![
        Line::from(Span::styled(SUN_GLYPH, bold(ACCENT))),
        Line::from(horizon),
        Line::default(),
        Line::from(Span::styled("dusk", bold(DUSK))),
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
    // The evening sky above the horizon, thinner than the header's.
    let over = Rect {
        height: top.saturating_sub(1),
        y: area.y - top,
        ..area
    };
    crate::ui::thread::sky::stars(frame, over, 31, None);
}

impl TranscriptView<'_> {
    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        self.app.viewport = area.height as usize;
        self.app.transcript_area = Some(area);
        self.app.transcript_hits.clear();
        self.app.previews.clear();
        if self.app.transcript.entries.is_empty() {
            self.app.scroll = 0;
            self.app.max_scroll = 0;
            welcome(frame, area);
            return;
        }
        self.app.transcript.set_live(self.live_step());
        let width = area.width as usize;
        let browsed = match self.app.focus {
            Focus::Browse(index) => Some(index),
            Focus::Input => None,
        };
        let last = self.app.transcript.entries.len() - 1;
        self.refresh_layout(width, browsed.filter(|&index| index < last));
        let Some(layout) = &self.app.layout else {
            return;
        };
        // Only the last entry can be in the middle of being written; it is drawn every frame.
        let earlier = layout.lines.len();
        let mut tail = Placed::default();
        let live = self.app.is_busy().then_some(&*self.app);
        let last_browsed = browsed.filter(|&index| index == last);
        append_entry(
            &self.app.transcript,
            last,
            width,
            live,
            last_browsed,
            &mut tail,
        );
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
        self.app.max_scroll = if height == 0 {
            0
        } else {
            total.saturating_sub(height)
        };
        // Scrolled up to read, the view stays on those lines while output grows below.
        let (drawn_total, drawn_width) = self.app.drawn_size;
        if self.app.scroll > FOLLOW_ROWS && width == drawn_width {
            self.app.scroll += total.saturating_sub(drawn_total);
        }
        self.app.drawn_size = (total, width);
        if let Focus::Browse(selected) = self.app.focus {
            self.keep_in_view(starts[selected], total, height);
        }
        self.app.scroll = self.app.scroll.min(self.app.max_scroll);
        let top = total.saturating_sub(height + self.app.scroll);
        for (index, &start) in starts.iter().enumerate() {
            if !self.app.transcript.is_foldable(index) {
                continue;
            }
            // A folded entry opens from its header; a message with a paste, from any row.
            let end = match self.app.transcript.entries[index] {
                Entry::User { .. } => starts.get(index + 1).copied().unwrap_or(total),
                _ => start + 1,
            };
            for line in start.max(top)..end.min(top + height) {
                self.app.transcript_hits.push(TranscriptHit {
                    row: area.y + (line - top) as u16,
                    left: area.x,
                    right: area.right(),
                    index,
                });
            }
        }
        let visible: Vec<Line> = (top..(top + height).min(earlier + tail.len()))
            .map(|line| {
                match self
                    .app
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
        let earlier_images = self
            .app
            .layout
            .as_ref()
            .map(|layout| layout.images.clone())
            .unwrap_or_default();
        self.app.previews = earlier_images
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
        self.app.drawn_top = top;
        self.app.tail_text = tail.iter().map(plain_text).collect();
        self.app.entry_lines = starts
            .iter()
            .zip(starts.iter().skip(1).chain([&total]))
            .map(|(&from, &to)| (from, to))
            .collect();
        frame.render_widget(Paragraph::new(visible), area);
        self.highlight(frame, area);
        self.buttons(frame, area);
    }

    /// Marks the selected text on screen, up to where each line's text ends.
    pub(super) fn highlight(&self, frame: &mut Frame, area: Rect) {
        let Some(selection) = self.app.selection else {
            return;
        };
        let (start, end) = selection.ordered();
        let text_left = area.x + TEXT_COLUMN;
        for row in 0..area.height {
            let line = self.app.drawn_top + row as usize;
            if line < start.0 || line > end.0 {
                continue;
            }
            let drawn = self.app.drawn_text(line).map_or(0, |text| text.width()) as u16;
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
    pub(super) fn buttons(&mut self, frame: &mut Frame, area: Rect) {
        self.app.button_hits.clear();
        let browsed = match self.app.focus {
            Focus::Browse(index) => Some(index),
            Focus::Input => None,
        };
        let Some(index) = self.app.hover.or(browsed) else {
            return;
        };
        let mut offered: Vec<(Button, &str, Color)> = match self.app.transcript.entries.get(index) {
            Some(Entry::Notice { .. } | Entry::Skill { .. }) | None => Vec::new(),
            Some(_) if self.app.transcript.is_hidden(index) => {
                vec![(Button::Copy, " copy ", TEXT_SOFT)]
            }
            Some(Entry::User { .. }) => vec![
                (Button::Hide, " hide ", TEXT_MUTED),
                (Button::Edit, " edit ", TEXT_SOFT),
                (Button::Copy, " copy ", TEXT_SOFT),
            ],
            Some(_) => vec![
                (Button::Hide, " hide ", TEXT_MUTED),
                (Button::Copy, " copy ", TEXT_SOFT),
            ],
        };
        // While its command runs, it can go on in the background; copy stays last.
        if self.app.transcript.running_command_at(index).is_some() {
            let before_copy = offered.len().saturating_sub(1);
            offered.insert(before_copy, (Button::Background, " background ", TEXT_SOFT));
        }
        let Some(&(from, to)) = self.app.entry_lines.get(index) else {
            return;
        };
        let line = from.max(self.app.drawn_top);
        if offered.is_empty() || line >= to || line >= self.app.drawn_top + area.height as usize {
            return;
        }
        let row = area.y + (line - self.app.drawn_top) as u16;
        // A message sits on a card narrower than the thread; its buttons end where it ends,
        // or they hang past its edge.
        let card_right = match self.app.transcript.entries.get(index) {
            Some(Entry::User { .. }) => self
                .app
                .drawn_text(line)
                .map(|text| area.x + text.width() as u16),
            _ => None,
        };
        let mut right = card_right.map_or(area.right(), |edge| edge.min(area.right()));
        for &(button, label, color) in offered.iter().rev() {
            let width = label.width() as u16;
            let Some(left) = right.checked_sub(width + 1).filter(|&left| left >= area.x) else {
                break;
            };
            let place = Rect::new(left, row, width + 1, 1);
            let shown = Line::from(vec![
                Span::raw(" "),
                Span::styled(label, fg(color).bg(RAISED)),
            ]);
            frame.render_widget(Paragraph::new(shown), place);
            self.app.button_hits.push(ButtonHit {
                row,
                left: left + 1,
                right: left + 1 + width,
                index,
                button,
            });
            right = left;
        }
        // Under the buttons, when the entry came.
        let below = row + 1;
        if let Some(at) = self.app.transcript.time(index)
            && below < area.bottom()
            && let Some(button) = self.app.button_hits.first()
        {
            let label = format!(" {} ", clock(at));
            let width = label.width() as u16;
            let left = button.right.saturating_sub(width).max(area.x);
            let place = Rect::new(left, below, width.min(area.right() - left), 1);
            let shown = Span::styled(label, fg(TEXT_MUTED).bg(RAISED));
            frame.render_widget(Paragraph::new(Line::from(shown)), place);
        }
    }

    /// Lays out every entry but the last; of those laid out on the last frame, only the
    /// ones from the first that changed since, so a new entry or a step that finished does
    /// not lay out the whole thread again. All of it when the width changed.
    pub(super) fn refresh_layout(&mut self, width: usize, browsed: Option<usize>) {
        let transcript = &self.app.transcript;
        let count = transcript.entries.len().saturating_sub(1);
        let signatures: Vec<u64> = (0..count)
            .map(|index| signature(transcript, index, browsed))
            .collect();
        let layout = self
            .app
            .layout
            .take()
            .filter(|layout| layout.width == width);
        let kept = layout.as_ref().map_or(0, |layout| {
            layout
                .signatures
                .iter()
                .zip(&signatures)
                .take_while(|(old, new)| old == new)
                .count()
        });
        let mut placed = Placed::default();
        let mut text = Vec::new();
        let mut before = Vec::new();
        if let Some(layout) = layout {
            let cut = layout
                .before
                .get(kept)
                .copied()
                .unwrap_or(layout.lines.len());
            placed.lines = layout.lines;
            placed.lines.truncate(cut);
            placed.starts = layout.starts;
            placed.starts.truncate(kept);
            placed.images = layout.images;
            placed.images.retain(|image| image.line < cut);
            text = layout.text;
            text.truncate(cut);
            before = layout.before;
            before.truncate(kept);
        }
        for index in kept..count {
            before.push(placed.lines.len());
            append_entry(transcript, index, width, None, browsed, &mut placed);
        }
        text.extend(placed.lines[text.len()..].iter().map(plain_text));
        let Placed {
            lines,
            starts,
            images,
        } = placed;
        self.app.layout = Some(Layout {
            width,
            signatures,
            text,
            lines,
            starts,
            before,
            images,
        });
    }

    /// The step the activity line names, while the agent works.
    pub(super) fn live_step(&self) -> Option<String> {
        if !self.app.is_busy() {
            return None;
        }
        match &self.app.activity {
            Activity::Idle => None,
            // The pace changes every moment, so it stays in the status line: a new live
            // step lays the transcript out again.
            Activity::Thinking => Some(self.app.thinking_label().into()),
            Activity::Writing => Some("writing".into()),
            Activity::Tool(name) => Some(name.clone()),
            Activity::Compacting => Some("compacting".into()),
            Activity::Forking => Some("forking".into()),
            // The attempt and the countdown live in the footer: a changing step would lay the
            // transcript out again.
            Activity::Retrying(..) => Some("retrying".into()),
        }
    }

    /// Scrolls just enough to show the selected entry.
    pub(super) fn keep_in_view(&mut self, start: usize, total: usize, height: usize) {
        let bottom = total.saturating_sub(self.app.scroll);
        let top = bottom.saturating_sub(height);
        if start < top {
            self.app.scroll = total.saturating_sub(start + height);
        } else if start >= bottom {
            self.app.scroll = total.saturating_sub(start + 1);
        }
    }
}

#[cfg(test)]
mod tests;
