//! Boxes drawn with line characters: the edges of a box with a title in its top border,
//! and text cut or padded to an exact number of columns. Widths are columns as the terminal
//! counts them, so wide characters (CJK, emoji) never push an edge out of place.

use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use super::{format::printable, wrap};

const HORIZONTAL: &str = "\u{2500}";
const VERTICAL: &str = "\u{2502}";

/// Columns a box spends on its two edges and the space inside each.
pub const FRAME: usize = 4;

/// The top edge, `width` columns wide, with `title` set into it: `╭─ title ────╮`. A title
/// too long for the edge is cut.
pub fn top(title: &str, width: usize, border: Style, title_style: Style) -> Line<'static> {
    let room = width.saturating_sub(FRAME + 1);
    let title = wrap::clip(&printable(title), room);
    let rule = width.saturating_sub(title.width() + FRAME + 1);
    Line::from(vec![
        Span::styled("\u{256D}\u{2500} ", border),
        Span::styled(title, title_style),
        Span::styled(format!(" {}\u{256E}", HORIZONTAL.repeat(rule)), border),
    ])
}

/// A top edge with no title: `╭────╮`.
pub fn top_plain(width: usize, border: Style) -> Line<'static> {
    let rule = HORIZONTAL.repeat(width.saturating_sub(2));
    Line::from(Span::styled(format!("\u{256D}{rule}\u{256E}"), border))
}

/// The bottom edge, `width` columns wide.
pub fn bottom(width: usize, border: Style) -> Line<'static> {
    let rule = HORIZONTAL.repeat(width.saturating_sub(2));
    Line::from(Span::styled(format!("\u{2570}{rule}\u{256F}"), border))
}

/// A row between the side edges, `width` columns wide in all. `content` is cut or padded
/// to fit, keeping its styles; it should hold no control characters.
pub fn row(content: Vec<Span<'static>>, width: usize, border: Style) -> Line<'static> {
    let room = width.saturating_sub(FRAME);
    let mut spans = vec![Span::styled(format!("{VERTICAL} "), border)];
    let mut used = 0;
    for span in content {
        let left = room - used;
        let fitted = if span.content.width() > left {
            wrap::clip(&span.content, left)
        } else {
            span.content.into_owned()
        };
        used += fitted.width();
        spans.push(Span::styled(fitted, span.style));
    }
    spans.push(Span::raw(" ".repeat(room - used)));
    spans.push(Span::styled(format!(" {VERTICAL}"), border));
    Line::from(spans)
}

/// How many boxes of `item` columns fit side by side in `room`, with `gap` between them;
/// at least one.
pub fn columns(room: usize, item: usize, gap: usize) -> usize {
    ((room + gap) / (item + gap)).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width(line: &Line) -> usize {
        line.spans.iter().map(|span| span.content.width()).sum()
    }

    #[test]
    fn every_edge_is_exactly_as_wide_as_asked_with_wide_text() {
        let style = Style::new();
        for size in [8, 24, 57, 100] {
            let title = "\u{6570}\u{636e}\u{1f680}".repeat(30);
            assert_eq!(width(&top(&title, size, style, style)), size);
            assert_eq!(width(&bottom(size, style)), size);
            let content = vec![Span::raw(title.clone())];
            assert_eq!(width(&row(content, size, style)), size);
        }
    }

    #[test]
    fn a_grid_has_at_least_one_column() {
        assert_eq!(columns(10, 24, 1), 1);
        assert_eq!(columns(56, 24, 1), 2);
        assert_eq!(columns(99, 24, 1), 4);
    }
}
