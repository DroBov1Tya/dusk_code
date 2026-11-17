//! The box that asks before a listing is installed, updated or removed: what it may do,
//! where it comes from and how far that is trusted, then the buttons.

use dusk_protocol::plugin::{Client, Kind};
use ratatui::{
    style::Style,
    text::{Line, Span},
};

use super::{mark, paragraph, word};
use crate::app::{Choice, Confirm, choices};
use crate::ui::text::format::one_line;
use crate::ui::windows::{
    theme::{ACCENT, BULLET, RAISED, bold, dim, fg},
    wrap,
};

/// What the change does and what the listing may do, then the buttons.
pub fn confirm_box(confirm: &Confirm, width: usize) -> Vec<Line<'static>> {
    let listing = &confirm.listing;
    let offered = choices(listing);
    let name = one_line(&listing.name);
    let version = one_line(&listing.version);
    let installed = listing
        .installed
        .as_deref()
        .map(one_line)
        .unwrap_or_default();
    let question = match offered.first() {
        Some(Choice::Install) => format!("Install {name} {version}?"),
        Some(Choice::Update) => {
            let newer = listing.update.as_deref().map_or(version, one_line);
            format!("Update {name} from {installed} to {newer}?")
        }
        _ => format!("Remove {name} {installed}?"),
    };
    let mut lines = paragraph(&question, bold(ACCENT), width);
    lines.extend(paragraph(&one_line(&listing.description), dim(), width));
    let (glyph, style) = mark(listing.tier);
    let source = match listing.source.as_str() {
        "" => "duskcode".to_owned(),
        source => one_line(source),
    };
    lines.push(Line::from(vec![
        Span::styled(format!("From {source} ("), dim()),
        Span::styled(format!("{glyph} {}", word(listing.tier)), style),
        Span::styled(")", dim()),
    ]));
    if listing.clash {
        lines.extend(paragraph(
            "Another source offers the same name. A mirror may copy a name to pass for \
             another source's listing; check that this is the one you mean.",
            fg(ACCENT),
            width,
        ));
    }
    lines.push(Line::default());
    if offered.contains(&Choice::Install) || offered.contains(&Choice::Update) {
        if listing.permissions.is_empty() {
            lines.extend(paragraph(
                "It asks for nothing beyond its own folder.",
                Style::new(),
                width,
            ));
        } else {
            lines.extend(paragraph("It may:", Style::new(), width));
            for permission in &listing.permissions {
                let text = format!("{BULLET} {}", one_line(permission));
                for line in wrap::words(vec![Span::raw(text)], width.saturating_sub(2), 2) {
                    lines.push(Line::from([vec![Span::raw(" ")], line].concat()));
                }
            }
        }
        if listing.kind != Kind::Plugin {
            lines.extend(paragraph(
                "Its skills and agents are text every thread may read; they run no programs.",
                dim(),
                width,
            ));
        }
        if !listing.clients.is_empty() && !listing.clients.contains(&Client::Tui) {
            lines.push(Line::default());
            lines.extend(paragraph(
                "Its panels do not show in the terminal.",
                fg(ACCENT),
                width,
            ));
        }
        lines.extend(paragraph("Installing switches it on.", dim(), width));
    }
    if offered.contains(&Choice::Remove) {
        lines.extend(paragraph(
            "Removing deletes its folder and everything in it.",
            dim(),
            width,
        ));
    }
    if let Some(homepage) = &listing.homepage {
        lines.extend(paragraph(
            &format!("More: {}", one_line(homepage)),
            dim(),
            width,
        ));
    }
    lines.push(Line::default());
    let mut buttons = vec![Span::raw(" ")];
    for (index, choice) in offered.iter().enumerate() {
        let style = match index == confirm.focus {
            true => bold(ACCENT).bg(RAISED),
            false => dim(),
        };
        buttons.push(Span::styled(format!(" {} ", choice.label()), style));
        buttons.push(Span::raw("  "));
    }
    lines.push(Line::from(buttons));
    lines
}
