//! Fitting styled text into a width. Words stay whole unless one alone is wider than the
//! line; code keeps every space and breaks anywhere.

use ratatui::text::Span;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Lines after the first start with `indent` spaces, so list items hang neatly.
pub fn words(spans: Vec<Span<'static>>, width: usize, indent: usize) -> Vec<Vec<Span<'static>>> {
    let width = width.max(indent + 1);
    let mut lines = Vec::new();
    let mut line = Vec::new();
    let mut used = 0;
    for span in spans {
        for piece in split_words(&span.content) {
            let piece_width = piece.width();
            let is_space = piece.starts_with(char::is_whitespace);
            if is_space && (used <= indent && !lines.is_empty() || used == 0) {
                continue;
            }
            if used + piece_width > width && used > indent {
                lines.push(std::mem::take(&mut line));
                line.push(Span::raw(" ".repeat(indent)));
                used = indent;
                if is_space {
                    continue;
                }
            }
            if piece_width <= width - used {
                line.push(Span::styled(piece.to_owned(), span.style));
                used += piece_width;
                continue;
            }
            // A word wider than the line is broken where it runs out.
            for c in piece.chars() {
                let char_width = c.width().unwrap_or(0);
                if used + char_width > width {
                    lines.push(std::mem::take(&mut line));
                    line.push(Span::raw(" ".repeat(indent)));
                    used = indent;
                }
                line.push(Span::styled(c.to_string(), span.style));
                used += char_width;
            }
        }
    }
    lines.push(line);
    lines
}

/// Breaks anywhere and keeps every character, for code.
pub fn chars(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = vec![String::new()];
    let mut used = 0;
    for c in text.chars() {
        let char_width = c.width().unwrap_or(0);
        if used + char_width > width {
            lines.push(String::new());
            used = 0;
        }
        if let Some(line) = lines.last_mut() {
            line.push(c);
        }
        used += char_width;
    }
    lines
}

/// Cuts text to `width` columns, ending with an ellipsis when it was longer.
pub fn clip(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let mut clipped = String::new();
    let mut used = 0;
    for c in text.chars() {
        let char_width = c.width().unwrap_or(0);
        if used + char_width + 1 > width {
            break;
        }
        clipped.push(c);
        used += char_width;
    }
    clipped.push_str(super::theme::ELLIPSIS);
    clipped
}

/// Runs of whitespace and runs of everything else, in order.
fn split_words(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut in_space = None;
    for (at, c) in text.char_indices() {
        let space = c.is_whitespace();
        if in_space.is_some_and(|was| was != space) {
            pieces.push(&text[start..at]);
            start = at;
        }
        in_space = Some(space);
    }
    if start < text.len() {
        pieces.push(&text[start..]);
    }
    pieces
}
