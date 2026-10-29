//! Answers as the model writes them: a little markdown, drawn instead of shown raw.
//! Headings, lists, quotes, `code`, **bold** and fenced blocks; the rest is plain text.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::{
    theme::{BULLET, CODE_BAR, GUTTER, ROSE, SUN, VIOLET, bold, dim, fg, thought},
    wrap,
};

const CODE: Color = Color::Rgb(206, 196, 240);

pub fn render(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code = false;
    for source in text.lines() {
        let trimmed = source.trim_start();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            for piece in wrap::chars(source, width.saturating_sub(2)) {
                let bar = Span::styled(format!("{CODE_BAR} "), dim());
                lines.push(Line::from(vec![bar, Span::styled(piece, fg(CODE))]));
            }
            continue;
        }
        if trimmed.is_empty() {
            lines.push(Line::default());
            continue;
        }
        let (prefix, indent, body, style) = classify(source);
        let mut spans = prefix;
        spans.extend(inline(body, style));
        lines.extend(
            wrap::words(spans, width, indent)
                .into_iter()
                .map(Line::from),
        );
    }
    lines
}

/// The marker a line starts with, how far its wrapped lines hang, the text after the
/// marker and the style of that text.
fn classify(source: &str) -> (Vec<Span<'static>>, usize, &str, Style) {
    let trimmed = source.trim_start();
    let lead = source.len() - trimmed.len();
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
        return (Vec::new(), 0, trimmed[hashes..].trim(), bold(VIOLET));
    }
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            let bullet = Span::styled(format!("{}{BULLET} ", " ".repeat(lead)), fg(ROSE));
            return (vec![bullet], lead + 2, rest, Style::new());
        }
    }
    if let Some(rest) = trimmed.strip_prefix("> ") {
        let bar = Span::styled(format!("{GUTTER} "), dim());
        return (vec![bar], 2, rest, thought());
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && trimmed[digits..].starts_with(". ") {
        let number = format!("{}{}. ", " ".repeat(lead), &trimmed[..digits]);
        let indent = number.len();
        return (
            vec![Span::styled(number, fg(SUN))],
            indent,
            &trimmed[digits + 2..],
            Style::new(),
        );
    }
    (Vec::new(), 0, source, Style::new())
}

/// `code` and **bold** inside a line.
fn inline(text: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = text;
    let mut strong = false;
    while !rest.is_empty() {
        let next = [rest.find('`'), rest.find("**")]
            .into_iter()
            .flatten()
            .min();
        let Some(at) = next else {
            spans.push(Span::styled(rest.to_owned(), emphasis(base, strong)));
            break;
        };
        if at > 0 {
            spans.push(Span::styled(rest[..at].to_owned(), emphasis(base, strong)));
        }
        rest = &rest[at..];
        if let Some(after) = rest.strip_prefix("**") {
            strong = !strong;
            rest = after;
            continue;
        }
        let code = &rest[1..];
        match code.find('`') {
            Some(end) => {
                spans.push(Span::styled(code[..end].to_owned(), fg(SUN)));
                rest = &code[end + 1..];
            }
            None => {
                spans.push(Span::styled(rest.to_owned(), emphasis(base, strong)));
                break;
            }
        }
    }
    spans
}

fn emphasis(base: Style, strong: bool) -> Style {
    if strong {
        base.add_modifier(Modifier::BOLD)
    } else {
        base
    }
}
