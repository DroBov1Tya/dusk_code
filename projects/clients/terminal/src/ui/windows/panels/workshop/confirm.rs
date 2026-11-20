//! The box that asks before a listing is installed, updated or removed: what it may do,
//! where it comes from and how far that is trusted, then the buttons.

use dusk_protocol::{
    catalog::DiffKind,
    plugin::{Client, Kind},
};
use ratatui::{
    style::Style,
    text::{Line, Span},
};

use super::{mark, paragraph, word};
use crate::app::{Choice, Confirm, Reviewed, choices};
use crate::ui::text::format::one_line;
use crate::ui::windows::{
    theme::{ACCENT, BULLET, NEGATIVE, POSITIVE, RAISED, WARNING, bold, dim, fg},
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
    if crate::app::reviewed_first(listing)
        && (offered.contains(&Choice::Install) || offered.contains(&Choice::Update))
    {
        lines.extend(unverified(confirm, width));
    } else if offered.contains(&Choice::Install) || offered.contains(&Choice::Update) {
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
        if let Some(server) = &listing.server {
            if let Some(docs) = &server.docs {
                lines.extend(paragraph(
                    &format!("Its docs: {}", one_line(docs)),
                    dim(),
                    width,
                ));
            }
            if !server.secrets.is_empty() {
                lines.extend(paragraph(
                    "It is added switched off; put the key in its settings, then switch it on.",
                    dim(),
                    width,
                ));
            }
        } else if listing.kind != Kind::Plugin {
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
        let switch = match listing.kind {
            Kind::Connector => "Installing adds its server to your connectors.",
            _ => "Installing switches it on.",
        };
        lines.extend(paragraph(switch, dim(), width));
    }
    if offered.contains(&Choice::Remove) {
        let removing = match listing.kind {
            Kind::Connector => "Removing takes its server out of your connectors.",
            _ => "Removing deletes its folder and everything in it.",
        };
        lines.extend(paragraph(removing, dim(), width));
    }
    if let Some(stars) = &listing.repo_stars {
        lines.extend(paragraph(&one_line(&stars.label()), dim(), width));
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

/// What an Unverified install is: where it comes from as far as anyone can tell, what it
/// may make the agent do, and every file it writes, with what an update changes.
fn unverified(confirm: &Confirm, width: usize) -> Vec<Line<'static>> {
    let listing = &confirm.listing;
    let from = match &listing.from {
        Some(host) => format!("{} ({})", one_line(&listing.source), one_line(host)),
        None => one_line(&listing.source),
    };
    let mut lines = paragraph(
        &format!(
            "Nobody vouches for {}. It comes from {from}, a source added without a key: \
             nobody signed its list and nobody checked these files.",
            one_line(&listing.name)
        ),
        fg(WARNING),
        width,
    );
    if let Some(upstream) = &listing.upstream {
        lines.extend(paragraph(
            &format!("Its files: {}.", one_line(upstream)),
            dim(),
            width,
        ));
    }
    if !listing.author.is_empty() {
        lines.extend(paragraph(
            &format!(
                "The source says it is by {}; nothing checks that.",
                one_line(&listing.author)
            ),
            dim(),
            width,
        ));
    }
    let review = match &confirm.review {
        None | Some(Reviewed::Reading) => {
            lines.extend(paragraph("Reading its files...", dim(), width));
            return lines;
        }
        Some(Reviewed::Failed(why)) => {
            lines.extend(paragraph(&one_line(why), fg(NEGATIVE), width));
            return lines;
        }
        Some(Reviewed::Read(review)) => review,
    };
    if let Some(why) = &review.refused {
        lines.extend(paragraph(&one_line(why), fg(NEGATIVE), width));
        return lines;
    }
    let what = match (review.agent, &review.agent_tools) {
        (true, None) => {
            "It is an agent: its AGENT.md becomes a chat's instructions, with every tool."
                .to_owned()
        }
        (true, Some(tools)) => format!(
            "It is an agent: its AGENT.md becomes a chat's instructions, with {}.",
            match tools.is_empty() {
                true => "no tools".to_owned(),
                false => one_line(&tools.join(", ")),
            }
        ),
        (false, _) => "Its text becomes instructions the agent may follow.".to_owned(),
    };
    lines.extend(paragraph(&what, Style::new(), width));
    if !review.scripts.is_empty() {
        lines.extend(paragraph(
            &format!(
                "It carries {} script(s) the agent may run, under the permission mode you chose: {}.",
                review.scripts.len(),
                one_line(&review.scripts.join(", "))
            ),
            fg(WARNING),
            width,
        ));
    }
    let update = !review.changed.is_empty() || !review.removed.is_empty();
    let groups = [
        ("Added", &review.added),
        ("Changed", &review.changed),
        ("Removed", &review.removed),
    ];
    for (title, paths) in groups {
        if paths.is_empty() || (!update && title != "Added") {
            continue;
        }
        let title = if update { title } else { "Files" };
        lines.push(Line::from(Span::styled(format!("{title}:"), bold(ACCENT))));
        for path in paths.iter().take(20) {
            lines.push(Line::from(format!("  {BULLET} {}", one_line(path))));
        }
        if paths.len() > 20 {
            lines.push(Line::from(Span::styled(
                format!("  and {} more", paths.len() - 20),
                dim(),
            )));
        }
    }
    if let Some(path) = &review.diff_path {
        lines.push(Line::from(Span::styled(
            format!("What changes in {}:", one_line(path)),
            bold(ACCENT),
        )));
        for line in review
            .diff
            .iter()
            .filter(|line| line.kind != DiffKind::Same)
            .take(30)
        {
            let (mark, color) = match line.kind {
                DiffKind::Added => ("+", POSITIVE),
                _ => ("-", NEGATIVE),
            };
            lines.push(Line::from(Span::styled(
                format!("  {mark} {}", one_line(&line.text)),
                fg(color),
            )));
        }
    }
    lines.extend(paragraph(
        "Installing it is your call: what it asks of the agent is yours to accept.",
        dim(),
        width,
    ));
    lines
}
