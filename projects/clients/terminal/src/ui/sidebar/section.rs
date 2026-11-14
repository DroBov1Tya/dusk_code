//! The pieces every section of the rail is built from: its heading, its rows and how far
//! it scrolls.

use unicode_width::UnicodeWidthStr;

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding},
};

use crate::ui::{
    text::wrap,
    theme::{DEEP, HAIRLINE, TEXT_SOFT, bold, fg},
};

/// Columns of air on each side of the rail's text.
pub const PAD: u16 = 2;
pub const GAUGE: &str = "\u{2501}";

/// A section's name with a hairline running from it to the right edge, where a value
/// may stand: the rail reads as blocks without a box around any of them.
pub fn ruled(name: &str, value: Option<Span<'static>>, width: usize) -> Line<'static> {
    let taken = name.width() + 1 + value.as_ref().map_or(0, |value| value.width() + 1);
    let mut spans = vec![
        Span::styled(name.to_owned(), bold(TEXT_SOFT)),
        Span::styled(
            format!(" {}", "\u{2500}".repeat(width.saturating_sub(taken))),
            fg(HAIRLINE),
        ),
    ];
    if let Some(value) = value {
        spans.push(Span::raw(" "));
        spans.push(value);
    }
    Line::from(spans)
}

/// `left` on the left and `right` against the right edge.
pub fn row(left: Vec<Span<'static>>, right: Span<'static>, width: usize) -> Line<'static> {
    let used: usize = left.iter().map(Span::width).sum::<usize>() + right.width();
    let mut spans = left;
    // At least one space, so a value never reads as part of its label.
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used).max(1))));
    spans.push(right);
    Line::from(spans)
}

/// A section below the statistics: its heading on the first row, then what it holds,
/// with the same air on both sides as the statistics.
pub fn section(title: Line<'static>) -> Block<'static> {
    let mut title = title;
    title.spans.insert(0, Span::raw(" ".repeat(PAD as usize)));
    Block::new()
        .style(Style::new().bg(DEEP))
        .title(title)
        .padding(Padding::horizontal(PAD))
}

/// `text` cut to `width` and padded to it, so what follows lines up on the right.
pub fn fill(text: &str, width: usize) -> String {
    let clipped = wrap::clip(text, width);
    let pad = width.saturating_sub(clipped.chars().count());
    format!("{clipped}{}", " ".repeat(pad))
}

/// Lines past the heading and, on overflow, the scroll hint below the body.
pub fn scroll_limit(line_count: usize, area: Rect) -> usize {
    let mut visible = area.height.saturating_sub(1) as usize;
    if line_count > visible && area.height > 2 {
        visible -= 1;
    }
    if visible == 0 {
        0
    } else {
        line_count.saturating_sub(visible).min(u16::MAX as usize)
    }
}
