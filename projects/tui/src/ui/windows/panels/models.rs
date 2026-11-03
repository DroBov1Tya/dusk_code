//! /model: the models of every provider.

use ratatui::text::{Line, Span};

use super::frame::selectable;
use crate::app::{ModelList, compact_number};
use crate::ui::windows::{
    theme::{ACCENT, ELLIPSIS, NEGATIVE, ON, POSITIVE, SUN_GLYPH, TEXT, bold, dim, fg, thought},
    wrap,
};

/// Providers with a light for whether they answered, each followed by its models. The
/// lines, and the line of the selected model.
pub fn models(list: &ModelList, width: usize) -> (Vec<Line<'static>>, usize) {
    let Some(providers) = &list.providers else {
        let checking = format!(" checking the providers {ELLIPSIS}");
        return (vec![Line::from(Span::styled(checking, thought()))], 0);
    };
    let mut lines = Vec::new();
    let mut selected_line = 0;
    let mut index = 0;
    for provider in providers {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        let (light, about) = match &provider.models {
            Ok(models) if models.len() == 1 => ((ON, POSITIVE), "1 model".into()),
            Ok(models) => ((ON, POSITIVE), format!("{} models", models.len())),
            Err(_) => ((ON, NEGATIVE), "unavailable".into()),
        };
        lines.push(selectable(
            vec![
                Span::styled(format!(" {} ", light.0), fg(light.1)),
                Span::styled(provider.name.clone(), bold(TEXT)),
            ],
            Span::styled(about, dim()),
            false,
        ));
        // Why a provider failed, in full under its name.
        if let Err(error) = &provider.models {
            lines.extend(
                wrap::words(vec![Span::styled(error.clone(), fg(NEGATIVE))], width, 0)
                    .into_iter()
                    .map(|spans| Line::from([vec![Span::raw("    ")], spans].concat())),
            );
        }
        for model in provider.models.as_deref().unwrap_or_default() {
            let name = format!("{}/{}", provider.name, model.id);
            let mark = if list.current.as_deref() == Some(name.as_str()) {
                Span::styled(format!("    {SUN_GLYPH} "), fg(ACCENT))
            } else {
                Span::raw("      ")
            };
            let window = model
                .context_window
                .map(|window| format!("{} tokens", compact_number(window)))
                .unwrap_or_default();
            if index == list.selected {
                selected_line = lines.len();
            }
            lines.push(selectable(
                vec![mark, Span::raw(model.id.clone())],
                Span::styled(window, dim()),
                index == list.selected,
            ));
            index += 1;
        }
    }
    if providers.is_empty() {
        lines.push(Line::from(Span::styled(
            " no providers yet; /connect adds one",
            thought(),
        )));
    }
    (lines, selected_line)
}
