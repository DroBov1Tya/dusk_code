//! The Workshop: the plugin store's list, and the box that asks before a change. Every
//! text of a listing comes from the network, so it is shown on one line, controls removed.

use dusk_protocol::plugin::{Client, Listing};
use ratatui::{
    style::Style,
    text::{Line, Span},
};

use super::frame::selectable;
use crate::app::{Choice, Confirm, Step, Workshop, choices};
use crate::ui::text::format::one_line;
use crate::ui::windows::{
    theme::{ACCENT, BULLET, ELLIPSIS, NEGATIVE, POSITIVE, RAISED, bold, dim, fg, thought},
    wrap,
};

/// The lines, and the line of the selected row.
pub fn workshop(view: &Workshop, width: usize) -> (Vec<Line<'static>>, usize) {
    let width = width.clamp(1, 70);
    if let Some(confirm) = &view.confirm {
        return (confirm_box(confirm, width), 0);
    }
    let mut lines = paragraph(
        "Plugins from the duskcode store; its list and every package are checked against \
         the key built into duskcode. Type to narrow the list.",
        dim(),
        width,
    );
    lines.push(Line::default());
    let mut filter = format!(" {}", view.filter);
    filter.push('\u{258F}');
    lines.push(Line::from(Span::styled(
        wrap::clip(&filter, width),
        fg(ACCENT).bg(RAISED),
    )));
    if let Some(step) = &view.step {
        let (text, style) = match step {
            Step::Working(what) => (format!("{what} {ELLIPSIS}"), thought()),
            Step::Done(what) => (what.clone(), fg(POSITIVE)),
            Step::Failed(what) => (what.clone(), fg(NEGATIVE)),
        };
        lines.push(Line::default());
        lines.extend(paragraph(&one_line(&text), style, width));
    }
    lines.push(Line::default());
    let Some(listings) = &view.listings else {
        let text = match view.step {
            Some(Step::Failed(_)) => " ctrl+r tries again".to_owned(),
            _ => format!(" Loading the store {ELLIPSIS}"),
        };
        lines.push(Line::from(Span::styled(text, thought())));
        return (lines, 0);
    };
    let shown = view.shown();
    if shown.is_empty() {
        let text = match listings.is_empty() {
            true => " The store offers no plugins yet",
            false => " No plugin matches",
        };
        lines.push(Line::from(Span::styled(text, thought())));
        return (lines, 0);
    }
    let mut row = 0;
    for (index, listing) in shown.into_iter().enumerate() {
        if index == view.selected {
            row = lines.len();
        }
        lines.extend(listing_rows(listing, index == view.selected, width));
    }
    (lines, row)
}

/// A listing: name and version with its state on the right, then what it is about.
fn listing_rows(listing: &Listing, selected: bool, width: usize) -> Vec<Line<'static>> {
    let (state, style) = match (&listing.installed, &listing.unfit) {
        (Some(installed), _) if *installed != listing.version && listing.unfit.is_none() => {
            (format!("update from {}", one_line(installed)), fg(ACCENT))
        }
        (Some(_), _) => ("installed".to_owned(), fg(POSITIVE)),
        (None, Some(_)) => ("unfit".to_owned(), fg(NEGATIVE)),
        (None, None) => (String::new(), dim()),
    };
    let name = format!(
        " {} {}",
        one_line(&listing.name),
        one_line(&listing.version)
    );
    let mut lines = vec![selectable(
        vec![Span::raw(name)],
        Span::styled(format!("{state} "), style),
        selected,
    )];
    let room = width.saturating_sub(3);
    let mut about = vec![(one_line(&listing.description), dim())];
    if let Some(reach) = reach(listing) {
        about.push((reach, dim()));
    }
    if let Some(unfit) = &listing.unfit {
        about.push((
            format!("cannot install here: {}", one_line(unfit)),
            fg(NEGATIVE),
        ));
    }
    for (text, style) in about {
        for line in wrap::words(vec![Span::styled(text, style)], room, 0) {
            lines.push(Line::from([vec![Span::raw("   ")], line].concat()));
        }
    }
    lines
}

/// The systems and clients it is for, when it is not for all.
fn reach(listing: &Listing) -> Option<String> {
    let platforms: Vec<_> = listing.platforms.iter().map(|os| os.name()).collect();
    let clients: Vec<_> = listing.clients.iter().map(|client| client.name()).collect();
    let parts: Vec<String> = [platforms, clients]
        .into_iter()
        .filter(|names| !names.is_empty())
        .map(|names| names.join(", "))
        .collect();
    (!parts.is_empty()).then(|| format!("for {}", parts.join("; ")))
}

/// What the change does and what the plugin may do, then the buttons.
fn confirm_box(confirm: &Confirm, width: usize) -> Vec<Line<'static>> {
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
        Some(Choice::Update) => format!("Update {name} from {installed} to {version}?"),
        _ => format!("Remove {name} {installed}?"),
    };
    let mut lines = paragraph(&question, bold(ACCENT), width);
    lines.extend(paragraph(&one_line(&listing.description), dim(), width));
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

fn paragraph(text: &str, style: Style, width: usize) -> Vec<Line<'static>> {
    wrap::words(vec![Span::styled(text.to_owned(), style)], width, 0)
        .into_iter()
        .map(Line::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::plugin::Os;
    use ratatui::{Terminal, backend::TestBackend, widgets::Paragraph};

    fn listing(name: &str) -> Listing {
        Listing {
            name: name.into(),
            version: "1.0.0".into(),
            description: format!("{name} does a thing"),
            platforms: Vec::new(),
            clients: Vec::new(),
            permissions: Vec::new(),
            homepage: None,
            installed: None,
            unfit: None,
        }
    }

    fn store() -> Vec<Listing> {
        let mut notes = listing("notes");
        notes.clients = vec![Client::Gui];
        notes.permissions = vec!["reach https://api.example.com".into()];
        let mut weather = listing("weather");
        weather.installed = Some("0.9.0".into());
        let mut screen = listing("screen");
        screen.platforms = vec![Os::Windows];
        screen.unfit = Some("only for windows".into());
        // A store text must not steer the terminal.
        screen.description = "grabs\x1b[2J the screen".into();
        vec![notes, weather, screen]
    }

    fn render(view: &Workshop) -> String {
        let (lines, _) = workshop(view, 70);
        let lines = super::super::frame::fit(lines, 72);
        let mut terminal = Terminal::new(TestBackend::new(72, 40)).expect("test terminal");
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new(lines), frame.area()))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn list_shows_state_reach_and_unfit_reason() {
        let view = Workshop {
            listings: Some(store()),
            ..Workshop::default()
        };
        let screen = render(&view);
        assert!(screen.contains("notes 1.0.0"), "{screen}");
        assert!(screen.contains("for gui"), "{screen}");
        assert!(screen.contains("update from 0.9.0"), "{screen}");
        assert!(screen.contains("unfit"), "{screen}");
        assert!(screen.contains("for windows"), "{screen}");
        assert!(
            screen.contains("cannot install here: only for windows"),
            "{screen}"
        );
        assert!(screen.contains("grabs [2J the screen"), "{screen}");
        assert!(!screen.contains('\x1b'));
    }

    #[test]
    fn loading_and_filter() {
        let mut view = Workshop::default();
        assert!(render(&view).contains("Loading the store"));
        view.listings = Some(store());
        view.filter = "weath".into();
        let screen = render(&view);
        assert!(screen.contains("weather"), "{screen}");
        assert!(!screen.contains("notes 1.0.0"), "{screen}");
    }

    #[test]
    fn confirm_lists_permissions_and_the_terminal_note() {
        let [notes, weather, screen] = <[Listing; 3]>::try_from(store()).expect("three");
        let view = Workshop {
            confirm: Some(Confirm {
                listing: notes,
                focus: 0,
            }),
            ..Workshop::default()
        };
        let text = render(&view);
        assert!(text.contains("Install notes 1.0.0?"), "{text}");
        assert!(text.contains("reach https://api.example.com"), "{text}");
        assert!(
            text.contains("Its panels do not show in the terminal."),
            "{text}"
        );
        assert!(
            text.contains("Install") && text.contains("Cancel"),
            "{text}"
        );

        let view = Workshop {
            confirm: Some(Confirm {
                listing: weather.clone(),
                focus: 0,
            }),
            ..Workshop::default()
        };
        let text = render(&view);
        assert!(
            text.contains("Update weather from 0.9.0 to 1.0.0?"),
            "{text}"
        );
        assert!(
            text.contains("It asks for nothing beyond its own folder."),
            "{text}"
        );
        assert!(!text.contains("do not show in the terminal"), "{text}");

        assert_eq!(
            choices(&weather),
            [Choice::Update, Choice::Remove, Choice::Cancel]
        );
        assert_eq!(choices(&screen), [Choice::Cancel]);
    }
}
