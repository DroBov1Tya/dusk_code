//! A source open to read: its name and size, then its text, markdown drawn and other text
//! as it is, cut to the window. Control characters are blanked, so a file cannot steer the
//! terminal; a picture or a binary file is named, not shown.

use ratatui::text::{Line, Span};

use dusk_protocol::thread::SourceKind;

use crate::app::{SourceReader, compact_number};
use crate::ui::text::format::printable;
use crate::ui::windows::{
    markdown,
    theme::{TEXT, bold, dim},
    wrap,
};

pub fn lines(reader: &SourceReader, width: usize) -> Vec<Line<'static>> {
    let source = &reader.source;
    let kind = match reader.kind {
        Some(SourceKind::Text) => "text",
        Some(SourceKind::Picture) => "picture",
        Some(SourceKind::Binary) => "binary file",
        None => "",
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" {}", wrap::clip(&printable(&source.name), width)),
                bold(TEXT),
            ),
            Span::styled(format!("  {} {kind}", compact_number(source.bytes)), dim()),
        ]),
        Line::raw(""),
    ];
    match reader.kind {
        Some(SourceKind::Text) => {
            let text = reader
                .text
                .lines()
                .map(printable)
                .collect::<Vec<_>>()
                .join("\n");
            let lower = source.name.to_ascii_lowercase();
            if lower.ends_with(".md") || lower.ends_with(".markdown") {
                lines.extend(markdown::render(&text, width));
            } else {
                for line in text.lines() {
                    for part in wrap::chars(line, width.saturating_sub(1)) {
                        lines.push(Line::raw(format!(" {part}")));
                    }
                }
            }
            if let Some(next) = reader.next {
                let left = compact_number(source.bytes.saturating_sub(next));
                let more = if reader.reads_on() {
                    format!(" {left} more: m reads on")
                } else {
                    format!(" {left} more is not shown here; open the file to read it all.")
                };
                lines.push(Line::raw(""));
                lines.push(Line::styled(more, dim()));
            }
        }
        Some(_) => lines.push(Line::styled(
            " Not text, so it is not shown here; open the file in its own app.",
            dim(),
        )),
        None => {}
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::thread::Source;

    /// A file is data: an escape sequence in it must not reach the terminal.
    #[test]
    fn control_characters_in_a_source_are_blanked() {
        let reader = SourceReader {
            sources: Vec::new(),
            selected: 0,
            source: Source {
                source_id: "s".into(),
                name: "log\u{1b}[2J.txt".into(),
                file: "s-log.txt".into(),
                bytes: 10,
                pinned: false,
                delivered: false,
            },
            kind: Some(SourceKind::Text),
            text: "ok\u{1b}]0;title\u{7}\nnext".into(),
            next: None,
            row: 0,
        };
        let drawn: String = lines(&reader, 60)
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.to_string()))
            .collect();
        assert!(!drawn.chars().any(char::is_control), "{drawn:?}");
        assert!(drawn.contains("next"));
    }

    /// A full reader stops reading on and says so, as the other clients do.
    #[test]
    fn a_full_reader_stops_at_its_bound() {
        let mut reader = SourceReader {
            sources: Vec::new(),
            selected: 0,
            source: Source {
                source_id: "s".into(),
                name: "big.txt".into(),
                file: "s-big.txt".into(),
                bytes: 4 * SourceReader::MAX_SHOWN as u64,
                pinned: false,
                delivered: false,
            },
            kind: Some(SourceKind::Text),
            text: "a".repeat(64),
            next: Some(64),
            row: 0,
        };
        assert!(reader.reads_on());
        reader.text = "a".repeat(SourceReader::MAX_SHOWN);
        assert!(!reader.reads_on());
        let drawn: String = lines(&reader, 60)
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.to_string()))
            .collect();
        assert!(drawn.contains("not shown here"), "{drawn:?}");
    }
}
