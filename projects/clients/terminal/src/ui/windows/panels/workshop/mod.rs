//! The Workshop: the store's list in sections by tier, a pack's page, and the box that
//! asks before a change. Every text of a listing comes from the network, so it is shown
//! on one line, controls removed. A tier is a glyph, a colour and its word, never colour
//! alone.

mod confirm;
mod pack;

use dusk_protocol::{
    catalog::SkillPack,
    plugin::{Kind, Listing, Order, Tier},
};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use unicode_width::UnicodeWidthStr;

use super::frame::selectable;
use crate::app::{Row, Step, Workshop, order_word};
use crate::ui::text::format::one_line;
use crate::ui::windows::{
    theme::{
        ACCENT, DONE, ELLIPSIS, INFO, NEGATIVE, POSITIVE, RAISED, TEXT_MUTED, TEXT_SOFT, bold, dim,
        fg, thought,
    },
    wrap,
};

/// The lines, and the line of the selected row.
pub fn workshop(view: &Workshop, width: usize) -> (Vec<Line<'static>>, usize) {
    let width = width.clamp(1, 100);
    if let Some(confirm) = &view.confirm {
        return (confirm::confirm_box(confirm, width), 0);
    }
    if let Some(page) = &view.pack {
        return pack::page(view, page, width);
    }
    let mut lines = paragraph(
        "Plugins, skills, packs and agents from the duskcode store and its mirrors; the \
         core checks every list and package. Type to narrow the list.",
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
    let category = view.category.as_deref().map_or("all".to_owned(), one_line);
    lines.push(Line::from(Span::styled(
        format!(" order: {}  category: {category}", order_word(view.order)),
        dim(),
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
    for failure in &view.failed {
        let text = format!(
            "{} could not be read: {}",
            one_line(&failure.source),
            one_line(&failure.error)
        );
        lines.extend(paragraph(&text, dim(), width));
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
            true => " The store offers nothing yet",
            false => " Nothing matches",
        };
        lines.push(Line::from(Span::styled(text, thought())));
        return (lines, 0);
    }
    let mut row = 0;
    let mut section = None;
    for (index, shown) in shown.into_iter().enumerate() {
        let heading = match (shown, view.order) {
            (Row::Listing(listing), Order::Trust) => Heading::Tier(listing.tier),
            (Row::Listing(_), order) => Heading::Order(order),
            (Row::Pack(_), _) => Heading::Packs,
        };
        if section != Some(heading) {
            if section.is_some() {
                lines.push(Line::default());
            }
            lines.extend(heading.lines(width));
            section = Some(heading);
        }
        let selected = index == view.selected;
        if selected {
            row = lines.len();
        }
        lines.extend(match shown {
            Row::Listing(listing) => listing_rows(listing, selected, width),
            Row::Pack(pack) => pack_rows(pack, selected, width),
        });
    }
    (lines, row)
}

/// A section: a tier while the list is in trust order, the order otherwise, then the
/// packs here. The rows stay in the core's order; a heading starts where the tier turns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Heading {
    Tier(Tier),
    Order(Order),
    Packs,
}

impl Heading {
    fn lines(self, width: usize) -> Vec<Line<'static>> {
        let (first, about) = match self {
            Self::Tier(tier) => {
                let (glyph, style) = mark(tier);
                let first = Line::from(vec![
                    Span::styled(format!(" {glyph} "), style),
                    Span::styled(word(tier), bold(color(tier))),
                ]);
                (first, about(tier))
            }
            Self::Order(order) => (
                Line::from(Span::styled(
                    format!(" {}", order_word(order)),
                    bold(TEXT_SOFT),
                )),
                "in the order the store gives",
            ),
            Self::Packs => {
                let (glyph, style) = mark(Tier::Local);
                let first = Line::from(vec![
                    Span::styled(format!(" {glyph} "), style),
                    Span::styled("Built-in and local packs", bold(TEXT_SOFT)),
                ]);
                (first, "skill packs here that no source lists")
            }
        };
        let mut lines = vec![first];
        lines.extend(indented(about, dim(), width));
        lines
    }
}

/// The glyph and colour of a tier; its word goes beside them.
pub fn mark(tier: Tier) -> (&'static str, Style) {
    (
        match tier {
            Tier::Official => DONE,
            Tier::Verified => "\u{25C6}",
            Tier::Community => "\u{25CB}",
            Tier::Local => "\u{25C7}",
        },
        fg(color(tier)),
    )
}

fn color(tier: Tier) -> Color {
    match tier {
        Tier::Official => ACCENT,
        Tier::Verified => INFO,
        Tier::Community => TEXT_MUTED,
        Tier::Local => TEXT_SOFT,
    }
}

pub fn word(tier: Tier) -> &'static str {
    match tier {
        Tier::Official => "Official",
        Tier::Verified => "Verified",
        Tier::Community => "Community",
        Tier::Local => "Local",
    }
}

fn about(tier: Tier) -> &'static str {
    match tier {
        Tier::Official => "signed by the key built into duskcode",
        Tier::Verified => "from mirrors the official list names, with their address and key",
        Tier::Community => "from mirrors you added; no key of duskcode vouches for them",
        Tier::Local => "found here; no source installed them",
    }
}

/// The width of the longest tag, `[plugin]`, so the states line up.
const KIND_WIDTH: usize = 8;

fn kind_tag(kind: Kind) -> &'static str {
    match kind {
        Kind::Plugin => "[plugin]",
        Kind::Skill => "[skill]",
        Kind::Pack => "[pack]",
        Kind::Agent => "[agent]",
    }
}

/// A listing: its mark, name and version with its kind and state on the right, then its
/// tier, and what it is about.
fn listing_rows(listing: &Listing, selected: bool, width: usize) -> Vec<Line<'static>> {
    let (state, style) = match (&listing.installed, &listing.update, &listing.unfit) {
        (Some(_), Some(update), None) => (format!("update to {}", one_line(update)), fg(ACCENT)),
        (Some(_), _, _) => ("installed".to_owned(), fg(POSITIVE)),
        (None, _, Some(_)) => ("unfit".to_owned(), fg(NEGATIVE)),
        (None, _, None) => (String::new(), dim()),
    };
    let (glyph, mark_style) = mark(listing.tier);
    let name = format!("{} {}", one_line(&listing.name), one_line(&listing.version));
    // The kind and the state are the right part; the name gives way to them.
    let right = format!("{:<KIND_WIDTH$}  {state} ", kind_tag(listing.kind));
    let name_room = width.saturating_sub(right.width() + 5);
    let mut lines = vec![selectable(
        vec![
            Span::styled(format!(" {glyph} "), mark_style),
            Span::raw(wrap::clip(&name, name_room)),
        ],
        Span::styled(right, style),
        selected,
    )];
    let mut facts = vec![word(listing.tier).to_owned()];
    if !listing.category.is_empty() {
        facts.push(one_line(&listing.category));
    }
    if listing.kind == Kind::Pack {
        facts.push(format!("{} skills, enter opens it", listing.skills.len()));
    }
    let mut about = vec![
        (facts.join(" \u{00B7} "), dim()),
        (one_line(&listing.description), dim()),
    ];
    if let Some(reach) = reach(listing) {
        about.push((reach, dim()));
    }
    if listing.clash {
        about.push(("another source offers the same name".to_owned(), fg(ACCENT)));
    }
    if let Some(unfit) = &listing.unfit {
        about.push((
            format!("cannot install here: {}", one_line(unfit)),
            fg(NEGATIVE),
        ));
    }
    for (text, style) in about {
        lines.extend(indented(&text, style, width));
    }
    lines
}

/// A pack here that no source lists: its name, how many skills are on, its switch.
fn pack_rows(pack: &SkillPack, selected: bool, width: usize) -> Vec<Line<'static>> {
    let (glyph, style) = mark(Tier::Local);
    let on = pack.skills.iter().filter(|skill| skill.enabled).count();
    let (state, state_style) = match pack.enabled {
        true => ("on", fg(POSITIVE)),
        false => ("off", dim()),
    };
    let mut lines = vec![selectable(
        vec![
            Span::styled(format!(" {glyph} "), style),
            Span::raw(one_line(&pack.name)),
        ],
        Span::styled(format!("{state} "), state_style),
        selected,
    )];
    let origin = if pack.builtin { "Built-in" } else { "Local" };
    let facts = format!(
        "{origin} \u{00B7} pack \u{00B7} {on} of {} skills on, enter opens it",
        pack.skills.len()
    );
    lines.extend(indented(&facts, dim(), width));
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

fn indented(text: &str, style: Style, width: usize) -> Vec<Line<'static>> {
    wrap::words(
        vec![Span::styled(text.to_owned(), style)],
        width.saturating_sub(3),
        0,
    )
    .into_iter()
    .map(|line| Line::from([vec![Span::raw("   ")], line].concat()))
    .collect()
}

fn paragraph(text: &str, style: Style, width: usize) -> Vec<Line<'static>> {
    wrap::words(vec![Span::styled(text.to_owned(), style)], width, 0)
        .into_iter()
        .map(Line::from)
        .collect()
}

#[cfg(test)]
mod tests;
