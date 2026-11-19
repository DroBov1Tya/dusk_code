//! A card of Workshop listings the agent put in a chat with `suggest_from_workshop`. The
//! core builds its rows from the store's own listings, never from what the model wrote,
//! and stores them in the call's result; a client draws the card from that result, live
//! and after a reload, and asks its own store list whether a row is installed, since
//! that changes after the card was shown. Adding goes through the Workshop's install and
//! its confirmation, never through the card alone.

use serde::{Deserialize, Serialize};

use super::{Cover, Kind, Listing, Scope, Tier};

/// The tool that finds listings for the model.
pub const SEARCH_WORKSHOP_TOOL: &str = "search_workshop";
/// The tool that shows a card of listings in the chat.
pub const SUGGEST_TOOL: &str = "suggest_from_workshop";
/// The most rows one card holds.
pub const MAX_SUGGESTED: usize = 6;
/// The longest heading of a card, in characters.
pub const MAX_LABEL_CHARS: usize = 80;

/// The card: a heading the model wrote, one plain line, and the listings it names.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    pub label: String,
    pub listings: Vec<Suggested>,
}

/// One row of a card: the listing as the store showed it when the card was made.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggested {
    pub source: String,
    pub name: String,
    pub kind: Kind,
    pub tier: Tier,
    pub version: String,
    pub description: String,
    /// How many skills it holds.
    #[serde(default)]
    pub skills: usize,
    #[serde(default)]
    pub cover: Option<Cover>,
}

impl Suggested {
    pub fn of(listing: &Listing) -> Self {
        Self {
            source: listing.source.clone(),
            name: listing.name.clone(),
            kind: listing.kind,
            tier: listing.tier,
            version: listing.version.clone(),
            description: listing.description.clone(),
            skills: listing.skills.len(),
            cover: listing.cover.clone(),
        }
    }

    /// The shelf whose Workshop lists the row: Plugins for a plugin, even one that brings
    /// a connector, since that is where it is installed and switched.
    pub fn scope(&self) -> Scope {
        let probe = Listing {
            kind: self.kind,
            ..Listing::default()
        };
        [Scope::Skills, Scope::Plugins, Scope::Agents]
            .into_iter()
            .find(|scope| scope.holds(&probe))
            .unwrap_or_default()
    }

    /// The row's listing in a store list, by its source and name.
    pub fn find<'a>(&self, listings: &'a [Listing]) -> Option<&'a Listing> {
        listings
            .iter()
            .find(|listing| listing.source == self.source && listing.name == self.name)
    }
}

/// How the model names a listing: `source/name`, as `search_workshop` lists it.
pub fn listing_id(listing: &Listing) -> String {
    format!("{}/{}", listing.source, listing.name)
}

/// The first line of the call's result, what the model reads before the card's data.
const SHOWN: &str = "The card is on the user's screen; they add from it themselves, through \
                     the Workshop's confirmation. Nothing was installed or switched on. \
                     Show no other card this turn.";

impl Suggestion {
    /// The call's result: a line for the model, then the card as JSON on the next.
    pub fn result(&self) -> String {
        // A card of plain strings always serializes.
        let json = serde_json::to_string(self).unwrap_or_default();
        format!("{SHOWN}\n{json}")
    }

    /// The card a stored or live result of `suggest_from_workshop` holds; none for an
    /// error or anything else. What follows the JSON, such as a note the core added to
    /// the stored result, is left alone.
    pub fn read(result: &str) -> Option<Self> {
        let json = result.strip_prefix(SHOWN)?.strip_prefix('\n')?;
        let card: Self = serde_json::Deserializer::from_str(json)
            .into_iter()
            .next()?
            .ok()?;
        let fits = !card.label.trim().is_empty()
            && card.label.chars().count() <= MAX_LABEL_CHARS + 3
            && (1..=MAX_SUGGESTED).contains(&card.listings.len());
        fits.then_some(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> Suggestion {
        let listing = Listing {
            name: "motion-kit".into(),
            source: "official".into(),
            version: "1.2.0".into(),
            description: "Interface animation.".into(),
            kind: Kind::Pack,
            skills: vec!["ease".into(), "spring".into()],
            ..Listing::default()
        };
        Suggestion {
            label: "Animation skills".into(),
            listings: vec![Suggested::of(&listing)],
        }
    }

    #[test]
    fn a_row_opens_on_the_shelf_of_its_kind() {
        let row = |kind| Suggested {
            kind,
            ..card().listings[0].clone()
        };
        assert_eq!(row(Kind::Pack).scope(), Scope::Skills);
        assert_eq!(row(Kind::Skill).scope(), Scope::Skills);
        assert_eq!(row(Kind::Plugin).scope(), Scope::Plugins);
        assert_eq!(row(Kind::Agent).scope(), Scope::Agents);
    }

    #[test]
    fn a_card_reads_back_from_its_result_and_nothing_else_does() {
        let shown = card();
        assert_eq!(shown.listings[0].skills, 2);
        assert_eq!(Suggestion::read(&shown.result()), Some(shown.clone()));
        // A note the core adds after the result does not hide the card.
        let noted = format!("{}\n\nnote: repeated call", shown.result());
        assert_eq!(Suggestion::read(&noted), Some(shown.clone()));
        assert_eq!(Suggestion::read("error: no such listing"), None);
        assert_eq!(
            Suggestion::read(&serde_json::to_string(&shown).unwrap()),
            None
        );
        let mut empty = shown.clone();
        empty.listings.clear();
        assert_eq!(Suggestion::read(&empty.result()), None);
        let mut many = shown.clone();
        many.listings = vec![shown.listings[0].clone(); MAX_SUGGESTED + 1];
        assert_eq!(Suggestion::read(&many.result()), None);
        let mut long = shown;
        long.label = "x".repeat(MAX_LABEL_CHARS + 4);
        assert_eq!(Suggestion::read(&long.result()), None);
    }
}
