//! /model: the models of every provider.

use ratatui::text::{Line, Span};

use super::frame::selectable;
use crate::app::{ModelList, compact_number};
use crate::ui::windows::{
    theme::{
        ACCENT, ELLIPSIS, NEGATIVE, OFF, ON, POSITIVE, SUN_GLYPH, TEXT, TEXT_MUTED, bold, dim, fg,
        thought,
    },
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
        let (light, mut about) = match &provider.models {
            _ if !provider.enabled => ((OFF, TEXT_MUTED), "off".to_owned()),
            Ok(models) if models.len() == 1 => ((ON, POSITIVE), "1 model".to_owned()),
            Ok(models) => ((ON, POSITIVE), format!("{} models", models.len())),
            Err(_) => ((ON, NEGATIVE), "unavailable".to_owned()),
        };
        match &provider.balance {
            Some(Ok(balance)) => about.push_str(&format!(" \u{00B7} {}", balance.text())),
            Some(Err(_)) => about.push_str(" \u{00B7} balance unknown"),
            None => {}
        }
        if index == list.selected {
            selected_line = lines.len();
        }
        let removing = list.removing.as_deref() == Some(provider.name.as_str());
        lines.push(selectable(
            vec![
                Span::styled(format!(" {} ", light.0), fg(light.1)),
                Span::styled(
                    provider.name.clone(),
                    if provider.enabled { bold(TEXT) } else { dim() },
                ),
            ],
            if removing {
                Span::styled("press d again to remove it".to_owned(), fg(NEGATIVE))
            } else {
                Span::styled(about, dim())
            },
            index == list.selected,
        ));
        index += 1;
        // Why a provider failed, in full under its name.
        if let Err(error) = &provider.models {
            lines.extend(
                wrap::words(vec![Span::styled(error.clone(), fg(NEGATIVE))], width, 0)
                    .into_iter()
                    .map(|spans| Line::from([vec![Span::raw("    ")], spans].concat())),
            );
        }
        if let Some(limits) = provider.limits.as_ref().filter(|_| provider.enabled) {
            let text = format!("Limits: {}", limits.text());
            lines.extend(
                wrap::words(vec![Span::styled(text, dim())], width, 0)
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
            " no providers yet; c connects one",
            thought(),
        )));
    }
    (lines, selected_line)
}
