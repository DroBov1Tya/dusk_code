//! A card of Workshop listings the agent put in the chat, as a short list: its heading,
//! a line per listing with its name, kind, tier and what it is, and how to add one from
//! the Workshop, which asks before it installs. Every text is the store's or the agent's,
//! so it is drawn on one line with controls removed.

use dusk_protocol::plugin::{Kind, Suggestion, Tier};
use ratatui::text::{Line, Span};

use crate::ui::text::format::one_line;
use crate::ui::thread::{
    theme::{DOT, GUTTER, TEXT_SOFT, TOOL, bold, dim, fg},
    wrap,
};

/// The card as lines `width` wide, past the entries' left margin.
pub(super) fn render(card: &Suggestion, width: usize) -> Vec<Line<'static>> {
    let room = width.saturating_sub(5).max(1);
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("   {TOOL} "), dim()),
        Span::styled(wrap::clip(&one_line(&card.label), room), bold(TEXT_SOFT)),
    ])];
    let row = |spans: Vec<Span<'static>>| {
        Line::from([vec![Span::styled(format!("   {GUTTER} "), dim())], spans].concat())
    };
    for listing in &card.listings {
        let name = one_line(&listing.name);
        let mut about = format!("{} {DOT} {}", kind(listing.kind), tier(listing.tier));
        // Nobody vouches for it, so where it comes from goes with it.
        if listing.tier == Tier::Unverified {
            about.push_str(&format!(" from {}", one_line(&listing.source)));
        }
        if listing.skills > 0 {
            about.push_str(&format!(" {DOT} {} skills", listing.skills));
        }
        let rest = format!("  {about}  {}", one_line(&listing.description));
        let rest_room = room.saturating_sub(name.chars().count());
        lines.push(row(vec![
            Span::styled(name, fg(TEXT_SOFT)),
            Span::styled(wrap::clip(&rest, rest_room), dim()),
        ]));
    }
    let hint = "Add one with /plugins store <name>; the Workshop asks before it installs.";
    lines.push(row(vec![Span::styled(wrap::clip(hint, room), dim())]));
    lines
}

fn kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Plugin => "plugin",
        Kind::Skill => "skill",
        Kind::Pack => "pack",
        Kind::Agent => "agent",
    }
}

fn tier(tier: Tier) -> &'static str {
    match tier {
        Tier::Official => "official",
        Tier::Verified => "verified",
        Tier::Community => "community",
        Tier::Unverified => "unverified",
        Tier::Local => "local",
    }
}
