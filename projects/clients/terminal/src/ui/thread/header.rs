//! The thread name sits quietly above the transcript; with more than one tab, the tabs
//! stand in its place.

use super::{
    theme::{
        ACCENT, BULLET, DOT, DUSK, HAIRLINE, POSITIVE, SUN_GLYPH, TEXT, TEXT_MUTED, bold, fg,
        thought,
    },
    wrap,
};
use crate::app::{App, TabHit};
use crate::ui::frame::TITLE_SPINNER;
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    let brand = vec![
        Span::styled(format!(" {SUN_GLYPH} "), bold(ACCENT)),
        Span::styled("dusk", bold(DUSK)),
        Span::styled(format!("  {DOT}  "), fg(HAIRLINE)),
    ];
    // The rail shows the effort; without it, the header does.
    let effort = if app.sidebar_visible && !app.compact_layout {
        String::new()
    } else {
        format!(
            " effort {} ",
            app.effort.map_or("default", |effort| effort.name())
        )
    };
    let effort_width = effort.width().min(area.width as usize);
    let title_area = Rect {
        width: area.width.saturating_sub(effort_width as u16),
        ..area
    };
    let brand_width: usize = brand.iter().map(|span| span.content.as_ref().width()).sum();
    let room = (title_area.width as usize).saturating_sub(brand_width);
    let mut spans = brand;
    app.tab_hits.clear();
    if app.strip().len() > 1 {
        let left = title_area.x + brand_width as u16;
        spans.extend(tabs(app, room, left, title_area));
    } else {
        // Said for as long as the chat is open: nothing of it is kept.
        if app.temporary || app.temporary_next {
            spans.push(Span::styled("temporary \u{00B7} ", thought()));
        }
        spans.push(match &app.title {
            Some(title) => Span::styled(wrap::clip(title, room), ratatui::style::Style::new()),
            None => Span::styled(wrap::clip("new thread", room), thought()),
        });
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), title_area);
    frame.render_widget(
        Paragraph::new(Span::styled(effort, thought())).right_aligned(),
        Rect {
            x: area.right().saturating_sub(effort_width as u16),
            width: effort_width as u16,
            ..area
        },
    );
}

/// Columns a tab takes besides its title: its number, the space after it, a mark and the
/// air before the next tab.
const TAB_FRAME: usize = 6;

/// The tabs, shown one in the accent; the others muted, with a mark when the agent works,
/// asks something or finished out of sight.
fn tabs(app: &mut App, room: usize, mut left: u16, area: Rect) -> Vec<Span<'static>> {
    let strip = app.strip();
    let each = (room / strip.len()).saturating_sub(TAB_FRAME).max(4);
    let mut spans = Vec::new();
    let mut hits = Vec::new();
    for (place, &index) in strip.iter().enumerate() {
        let shown = index == app.tab;
        let tab = &app.tabs[index];
        let title = match shown {
            true => app.title.as_deref(),
            false => tab.title.as_deref(),
        };
        let label = format!(
            "{} {}",
            place + 1,
            wrap::clip(title.unwrap_or("new thread"), each)
        );
        let mark = match () {
            _ if shown => None,
            _ if tab.asks() => Some(Span::styled("?", bold(TEXT))),
            _ if tab.working() => Some(Span::styled(
                TITLE_SPINNER[(app.frame / 2) % TITLE_SPINNER.len()],
                fg(DUSK),
            )),
            _ if tab.unseen => Some(Span::styled(BULLET, fg(POSITIVE))),
            _ => None,
        };
        let width = (label.width() + mark.as_ref().map_or(0, |mark| mark.width() + 1)) as u16;
        if left < area.right() {
            hits.push(TabHit {
                row: area.y,
                left,
                right: (left + width).min(area.right()),
                place,
            });
        }
        let style = if shown { bold(ACCENT) } else { fg(TEXT_MUTED) };
        spans.push(Span::styled(label, style));
        if let Some(mark) = mark {
            spans.push(Span::raw(" "));
            spans.push(mark);
        }
        spans.push(Span::raw("  "));
        left = left.saturating_add(width + 2);
    }
    app.tab_hits = hits;
    spans
}
