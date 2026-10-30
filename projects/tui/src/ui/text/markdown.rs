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
    let source_lines: Vec<&str> = text.lines().collect();
    let mut row = 0;
    while row < source_lines.len() {
        let source = source_lines[row];
        row += 1;
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
        if let Some((table, consumed)) = table(&source_lines[row - 1..], width) {
            lines.extend(table);
            row += consumed - 1;
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

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

/// A header followed by a Markdown delimiter row is a table. An incomplete table
/// during streaming stays text until its delimiter arrives.
fn table(source: &[&str], width: usize) -> Option<(Vec<Line<'static>>, usize)> {
    let header = cells(source.first()?);
    let delimiter = cells(source.get(1)?);
    if header.is_empty() || header.len() > 32 || header.len() != delimiter.len() {
        return None;
    }
    let alignment: Option<Vec<Align>> = delimiter
        .iter()
        .map(|cell| {
            let dashes = cell.trim_matches(':');
            if dashes.len() < 3 || !dashes.bytes().all(|c| c == b'-') {
                return None;
            }
            Some(match (cell.starts_with(':'), cell.ends_with(':')) {
                (true, true) => Align::Center,
                (_, true) => Align::Right,
                _ => Align::Left,
            })
        })
        .collect();
    let alignment = alignment?;
    let mut rows = vec![header];
    let mut consumed = 2;
    for source in &source[2..] {
        if source.trim().is_empty()
            || !source.contains('|')
            || source.trim_start().starts_with("```")
        {
            break;
        }
        let mut row = cells(source);
        // A non-table line is left for the ordinary renderer; data is never dropped.
        if row.len() > rows[0].len() {
            break;
        }
        row.resize(rows[0].len(), String::new());
        rows.push(row);
        consumed += 1;
    }
    let columns = alignment.len();
    let separators = columns.saturating_sub(1) * 3;
    if width < columns * 3 + separators {
        return Some((stacked_table(&rows, width), consumed));
    }
    let available = width - separators;
    let mut widths = vec![3; columns];
    let natural: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .map(|row| {
                    inline(&row[column], Style::new())
                        .iter()
                        .map(Span::width)
                        .sum()
                })
                .max()
                .unwrap_or(3)
                .max(3)
        })
        .collect();
    let mut remaining = available - columns * 3;
    while remaining > 0 {
        let Some(column) = (0..columns)
            .filter(|&column| widths[column] < natural[column])
            .max_by_key(|&column| natural[column] - widths[column])
        else {
            break;
        };
        widths[column] += 1;
        remaining -= 1;
    }
    let mut lines = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let base = if index == 0 {
            bold(VIOLET)
        } else {
            Style::new()
        };
        let wrapped: Vec<_> = row
            .iter()
            .zip(&widths)
            .map(|(cell, &width)| wrap::words(inline(cell, base), width, 0))
            .collect();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        for line in 0..height {
            let mut spans = Vec::new();
            for column in 0..columns {
                if column > 0 {
                    spans.push(Span::styled(" \u{2502} ", dim()));
                }
                let content = wrapped[column].get(line).cloned().unwrap_or_default();
                let used: usize = content.iter().map(Span::width).sum();
                let padding = widths[column].saturating_sub(used);
                let left = match alignment[column] {
                    Align::Left => 0,
                    Align::Center => padding / 2,
                    Align::Right => padding,
                };
                spans.push(Span::raw(" ".repeat(left)));
                spans.extend(content);
                spans.push(Span::raw(" ".repeat(padding - left)));
            }
            lines.push(Line::from(spans));
        }
        if index == 0 {
            lines.push(Line::from(Span::styled(
                widths
                    .iter()
                    .map(|&width| "\u{2500}".repeat(width))
                    .collect::<Vec<_>>()
                    .join("\u{2500}\u{253c}\u{2500}"),
                dim(),
            )));
        }
    }
    Some((lines, consumed))
}

fn stacked_table(rows: &[Vec<String>], width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for row in rows.iter().skip(1) {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        for (header, cell) in rows[0].iter().zip(row) {
            let mut spans = inline(header, bold(VIOLET));
            spans.push(Span::raw(": "));
            spans.extend(inline(cell, Style::new()));
            lines.extend(wrap::words(spans, width, 0).into_iter().map(Line::from));
        }
    }
    if rows.len() == 1 {
        for header in &rows[0] {
            lines.extend(
                wrap::words(inline(header, bold(VIOLET)), width, 0)
                    .into_iter()
                    .map(Line::from),
            );
        }
    }
    lines
}

/// Pipes escaped with a backslash or contained in a backtick code span are content.
fn cells(source: &str) -> Vec<String> {
    let source = source.trim();
    if !source.contains('|') {
        return Vec::new();
    }
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = source.chars().peekable();
    let mut code_ticks = 0;
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            chars.next();
            cell.push('|');
        } else if c == '`' {
            let mut count = 1;
            cell.push(c);
            while chars.peek() == Some(&'`') {
                chars.next();
                cell.push('`');
                count += 1;
            }
            if code_ticks == 0 {
                code_ticks = count;
            } else if code_ticks == count {
                code_ticks = 0;
            }
        } else if c == '|' && code_ticks == 0 {
            cells.push(cell.trim().to_owned());
            cell.clear();
        } else {
            cell.push(c);
        }
    }
    cells.push(cell.trim().to_owned());
    if source.starts_with('|') {
        cells.remove(0);
    }
    if source.ends_with('|') && cells.last().is_some_and(String::is_empty) {
        cells.pop();
    }
    cells
}

/// Deeper or longer markers are plain text: nesting is never that deep in real answers.
const MAX_LEAD: usize = 16;
const MAX_DIGITS: usize = 9;

/// The marker a line starts with, how far its wrapped lines hang, the text after the
/// marker and the style of that text.
fn classify(source: &str) -> (Vec<Span<'static>>, usize, &str, Style) {
    let trimmed = source.trim_start();
    let lead = source.len() - trimmed.len();
    if lead > MAX_LEAD {
        return (Vec::new(), 0, source, Style::new());
    }
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
    if (1..=MAX_DIGITS).contains(&digits) && trimmed[digits..].starts_with(". ") {
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
