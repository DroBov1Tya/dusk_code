//! /diff: the changes of this chat's folder. A list of files with added and removed
//! lines; Enter opens one as numbered green, red and grey lines.

use ratatui::text::{Line, Span};

use dusk_protocol::project::{ChangeKind, DiffKind};

use super::frame::selectable;
use crate::app::DiffView;
use crate::ui::text::format::printable;
use crate::ui::windows::{
    theme::{ACCENT, NEGATIVE, POSITIVE, TEXT_MUTED, bold, dim, fg},
    wrap,
};

pub fn list(view: &DiffView, width: usize) -> Vec<Line<'static>> {
    view.files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let counts = format!("+{} \u{2212}{} ", file.added, file.removed);
            selectable(
                vec![
                    Span::styled(
                        format!(" {} ", letter(file.kind)),
                        fg(kind_color(file.kind)),
                    ),
                    Span::raw(wrap::clip(&file.path, width.saturating_sub(18))),
                ],
                Span::styled(counts, dim()),
                index == view.selected,
            )
        })
        .collect()
}

/// One file: its name, then every hunk with the line numbers of both sides.
pub fn file(view: &DiffView, width: usize) -> Vec<Line<'static>> {
    let Some(file) = view.files.get(view.selected) else {
        return Vec::new();
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(format!(" {} ", file.path), bold(ACCENT)),
        Span::styled(format!("+{}", file.added), fg(POSITIVE)),
        Span::raw(" "),
        Span::styled(format!("\u{2212}{}", file.removed), fg(NEGATIVE)),
    ])];
    if file.hunks.is_empty() {
        let note = if file.binary {
            "binary, or too large to show"
        } else {
            "no lines to show"
        };
        lines.push(Line::from(Span::styled(format!(" {note}"), dim())));
    }
    let room = width.saturating_sub(14);
    for hunk in &file.hunks {
        lines.push(Line::from(Span::styled(
            format!(" {}", wrap::clip(&hunk.header, width.saturating_sub(2))),
            fg(TEXT_MUTED),
        )));
        for line in &hunk.lines {
            let (sign, color) = match line.kind {
                DiffKind::Added => ('+', Some(POSITIVE)),
                DiffKind::Removed => ('\u{2212}', Some(NEGATIVE)),
                DiffKind::Context => (' ', None),
            };
            let number = |n: Option<u32>| n.map_or("    ".to_owned(), |n| format!("{n:>4}"));
            let text = wrap::clip(&printable(&line.text), room);
            let body = match color {
                Some(color) => Span::styled(format!("{sign} {text}"), fg(color)),
                None => Span::styled(format!("{sign} {text}"), fg(TEXT_MUTED)),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} {} ", number(line.old), number(line.new)),
                    dim(),
                ),
                body,
            ]));
        }
    }
    lines
}

fn letter(kind: ChangeKind) -> char {
    match kind {
        ChangeKind::Added => 'A',
        ChangeKind::Modified => 'M',
        ChangeKind::Deleted => 'D',
        ChangeKind::Renamed => 'R',
        ChangeKind::Untracked => 'U',
        ChangeKind::Conflicted => '!',
    }
}

fn kind_color(kind: ChangeKind) -> ratatui::style::Color {
    match kind {
        ChangeKind::Added | ChangeKind::Untracked => POSITIVE,
        ChangeKind::Deleted | ChangeKind::Conflicted => NEGATIVE,
        _ => ACCENT,
    }
}
