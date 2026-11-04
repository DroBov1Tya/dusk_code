//! /plugins: the plugins found and whether each is on.

use ratatui::text::{Line, Span};

use super::frame::selectable;
use crate::app::{App, SecretEntry};
use crate::ui::windows::{
    theme::{ACCENT, NEGATIVE, RAISED, dim, fg, thought},
    wrap,
};

pub fn plugins(
    app: &App,
    selected: usize,
    entry: Option<&SecretEntry>,
    width: usize,
) -> (Vec<Line<'static>>, usize) {
    let width = width.clamp(1, 70);
    if let Some(entry) = entry {
        return (secret_form(entry, width), 0);
    }
    let mut lines: Vec<Line<'static>> = wrap::words(
        vec![Span::styled(
            "A plugin is a folder with a plugin.yml in ~/.config/duskcode/rules/plugins/. A \
             found one stays off until it is switched on here, since it runs programs of its \
             own; its settings go under plugins: in config.yml. Secrets such as a bot token are \
             typed here with s and kept in memory for this session only.",
            dim(),
        )],
        width,
        0,
    )
    .into_iter()
    .map(Line::from)
    .collect();
    lines.push(Line::default());
    if app.plugins.is_empty() {
        lines.push(Line::from(Span::styled(" No plugins found", thought())));
        return (lines, 0);
    }
    let mut row = 0;
    for (index, plugin) in app.plugins.iter().enumerate() {
        if index == selected {
            row = lines.len();
        }
        let state = if plugin.enabled { "on" } else { "off" };
        let version = if plugin.version.is_empty() {
            String::new()
        } else {
            format!(" {}", plugin.version)
        };
        lines.push(selectable(
            vec![Span::raw(format!(" {}{version}", plugin.name))],
            Span::styled(format!("{state} "), dim()),
            index == selected,
        ));
        let mut about = plugin.description.clone();
        if !plugin.parts.is_empty() {
            about.push_str(&format!(" ({})", plugin.parts.join(", ")));
        }
        if let Some(note) = &plugin.note {
            about.push_str(&format!(" \u{2014} {note}"));
        }
        for line in wrap::words(vec![Span::styled(about, dim())], width.saturating_sub(3), 0) {
            lines.push(Line::from([vec![Span::raw("   ")], line].concat()));
        }
        for (key, set) in &plugin.secrets {
            let status = if *set {
                "session only, not saved"
            } else {
                "not set; press s to enter"
            };
            lines.push(Line::from(Span::styled(
                format!("   {key}: {status}"),
                dim(),
            )));
        }
    }
    (lines, row)
}

/// The masked field: stars only, never the text.
fn secret_form(entry: &SecretEntry, width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = wrap::words(
        vec![Span::styled(
            format!(
                "{} / {}: kept in memory for this session only, never written to a file, \
                 and gone when duskcode closes.",
                entry.plugin, entry.key
            ),
            dim(),
        )],
        width,
        0,
    )
    .into_iter()
    .map(Line::from)
    .collect();
    lines.push(Line::default());
    let at = entry.input.text[..entry.input.cursor].chars().count();
    let total = entry.input.text.chars().count();
    let mut value = "*".repeat(at);
    value.push('\u{258F}');
    value.push_str(&"*".repeat(total - at));
    lines.push(Line::from(Span::styled(
        format!(" {}", wrap::clip(&value, width)),
        fg(ACCENT).bg(RAISED),
    )));
    if let Some(error) = &entry.error {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(error.clone(), fg(NEGATIVE))));
    }
    lines
}
