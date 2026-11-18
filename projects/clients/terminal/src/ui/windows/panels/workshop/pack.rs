//! A pack's page: where it comes from, its own switch, and its skills with theirs. A
//! pack not yet here lists its skills by name and waits for `i`.

use dusk_protocol::plugin::Tier;
use ratatui::text::{Line, Span};

use super::super::frame::selectable;
use super::{indented, mark, paragraph, word};
use crate::app::{PackPage, Workshop};
use crate::ui::text::format::one_line;
use crate::ui::windows::theme::{ACCENT, OFF, ON, POSITIVE, bold, dim, fg};

/// The lines, and the line of the selected row.
pub fn page(view: &Workshop, page: &PackPage, width: usize) -> (Vec<Line<'static>>, usize) {
    let listing = view.pack_listing(&page.name);
    let here = view.pack_here(&page.name);
    let title = here
        .map(|pack| pack.title.as_str())
        .filter(|title| !title.is_empty())
        .unwrap_or(&page.name);
    let tier = listing.map_or(Tier::Local, |listing| listing.tier);
    let (glyph, style) = mark(tier);
    let origin = match (listing, here) {
        (None, Some(pack)) if pack.builtin => "Built-in".to_owned(),
        _ => word(tier).to_owned(),
    };
    let mut head = vec![
        Span::styled(format!("{} ", one_line(title)), bold(ACCENT)),
        Span::styled(format!("{glyph} {origin}"), style),
    ];
    if let Some(listing) = listing {
        head.push(Span::styled(
            format!("  {}", one_line(&listing.version)),
            dim(),
        ));
    }
    let mut lines = vec![Line::from(head)];
    let description = listing
        .map(|listing| listing.description.as_str())
        .or(here.map(|pack| pack.description.as_str()))
        .unwrap_or_default();
    if !description.is_empty() {
        lines.extend(paragraph(&one_line(description), dim(), width));
    }
    if let Some(listing) = listing {
        let state = match (&listing.installed, &listing.update) {
            (None, _) => "not installed; i installs it".to_owned(),
            (Some(_), Some(update)) => format!("update to {}; i updates it", one_line(update)),
            (Some(_), None) => "installed".to_owned(),
        };
        lines.extend(paragraph(&state, dim(), width));
    }
    lines.push(Line::default());
    let Some(pack) = here else {
        let skills = listing
            .map(|listing| listing.skills.as_slice())
            .unwrap_or_default();
        lines.extend(paragraph(
            "Install the pack to switch its skills one by one:",
            dim(),
            width,
        ));
        for skill in skills {
            lines.extend(indented(&one_line(skill), dim(), width));
        }
        return (lines, 0);
    };
    let switch = |on: bool| match on {
        true => Span::styled(format!("{ON} on "), fg(POSITIVE)),
        false => Span::styled(format!("{OFF} off "), dim()),
    };
    let mut row = lines.len();
    lines.push(selectable(
        vec![Span::raw(" the pack")],
        switch(pack.enabled),
        page.selected == 0,
    ));
    if !pack.enabled {
        lines.extend(indented(
            "off: its skills are hidden whatever their own switches say",
            dim(),
            width,
        ));
    }
    for (index, skill) in pack.skills.iter().enumerate() {
        let short = skill
            .name
            .strip_prefix(&format!("{}/", pack.name))
            .unwrap_or(&skill.name);
        if page.selected == index + 1 {
            row = lines.len();
        }
        lines.push(selectable(
            vec![Span::raw(format!("   {}", one_line(short)))],
            switch(skill.enabled),
            page.selected == index + 1,
        ));
    }
    (lines, row)
}
