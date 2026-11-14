//! /connect: a provider and the fields it needs.

use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::app::{Connection, Preset};
use crate::ui::windows::{
    theme::{ACCENT, DUSK, NEGATIVE, RAISED, bold, dim, fg},
    wrap,
};

/// Connection details stay in this form and are never echoed into the transcript.
pub fn connection(form: &Connection, width: usize) -> (Vec<Line<'static>>, usize, Vec<usize>) {
    let field_width = width.saturating_sub(4).clamp(1, 68);
    let words = |text: &str, style: Style| -> Vec<Line<'static>> {
        wrap::words(vec![Span::styled(text.to_owned(), style)], field_width, 0)
            .into_iter()
            .map(Line::from)
            .collect()
    };
    let Some(preset) = form.preset else {
        // The providers first; each opens a form of only what it needs.
        let mut lines = words("Pick a provider.", dim());
        lines.push(Line::default());
        let (mut rows, mut selected) = (Vec::new(), 0);
        for (index, preset) in Preset::ALL.into_iter().enumerate() {
            rows.push(lines.len());
            let style = if index == form.choice {
                selected = lines.len();
                fg(ACCENT).bg(RAISED)
            } else {
                Style::new()
            };
            lines.push(Line::from(Span::styled(
                format!(" {}", preset.label()),
                style,
            )));
            let about = format!(" {}", preset.about());
            lines.extend(
                wrap::words(vec![Span::styled(about, dim())], field_width, 1)
                    .into_iter()
                    .map(Line::from),
            );
            lines.push(Line::default());
        }
        return (lines, selected, rows);
    };
    let mut lines = vec![Line::from(Span::styled(
        format!(" {}", preset.label()),
        bold(DUSK),
    ))];
    lines.extend(words(preset.about(), dim()));
    if let Some(page) = preset.key_page() {
        lines.extend(words(
            &format!("Make a key at {page}; ctrl+o opens it."),
            dim(),
        ));
    }
    lines.push(Line::default());
    let key_label = match preset.needs_key() {
        true => "API key",
        false => "API key (optional)",
    };
    let labels = ["name (optional)", "base URL", key_label];
    let mut rows = Vec::new();
    let mut selected = 0;
    for (index, field) in form.fields.iter().enumerate() {
        rows.push(lines.len());
        lines.push(Line::from(Span::styled(
            format!(" {}", labels[index]),
            dim(),
        )));
        let at = field.text[..field.cursor].chars().count();
        let chars: Vec<char> = if index == 2 {
            vec!['*'; field.text.chars().count()]
        } else {
            field.text.chars().collect()
        };
        let start = at.saturating_sub(field_width / 2);
        let mut value = String::new();
        if start > 0 {
            value.push('\u{2026}');
        }
        for position in start..=chars.len() {
            if position == at && index == form.selected {
                value.push('\u{258F}');
            }
            if let Some(c) = chars.get(position) {
                value.push(*c);
            }
        }
        if value.is_empty() {
            value.push_str(match index {
                0 => "provider-N when left blank",
                1 => "required",
                _ if preset.needs_key() => "required",
                _ => "leave blank if not needed",
            });
        }
        if index == form.selected {
            selected = lines.len();
        }
        let style = if index == form.selected {
            fg(ACCENT).bg(RAISED)
        } else {
            Style::new()
        };
        lines.push(Line::from(Span::styled(
            format!(" {}", wrap::clip(&value, field_width)),
            style,
        )));
        lines.push(Line::default());
    }
    if let Some(error) = &form.error {
        lines.extend(words(error, fg(NEGATIVE)));
    }
    (lines, selected, rows)
}
