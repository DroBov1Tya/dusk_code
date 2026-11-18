//! A listing's byline, "by <author> · <version> · N skills", and on its page
//! "· updated <date>" after it; a card has no room for the date. A part the list does not
//! give is left out, never guessed.

use dusk_protocol::plugin::{Kind, Listing};

use super::capsule;
use crate::ui::widgets;

/// The longest author drawn; the list allows 64 characters.
const AUTHOR_CHARS: usize = 64;

/// The card's line.
pub fn byline(listing: &Listing) -> String {
    parts(listing).join(" \u{b7} ")
}

/// The page's line: the card's, then when it was updated.
pub fn dated_byline(listing: &Listing) -> String {
    let mut parts = parts(listing);
    if let Some(date) = day(listing.updated.as_deref()) {
        parts.push(format!("updated {date}"));
    }
    parts.join(" \u{b7} ")
}

fn parts(listing: &Listing) -> Vec<String> {
    let mut parts = Vec::new();
    if !listing.author.is_empty() {
        parts.push(format!(
            "by {}",
            capsule::clip(&listing.author, AUTHOR_CHARS)
        ));
    }
    if !listing.version.is_empty() {
        parts.push(format!("v{}", capsule::clip(&listing.version, 40)));
    }
    if listing.kind == Kind::Pack && !listing.skills.is_empty() {
        parts.push(widgets::count(listing.skills.len(), "skill"));
    }
    parts
}

/// The day of an RFC 3339 time in words, as "11 Nov 2026"; none when it is not a day.
pub fn day(text: Option<&str>) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let date = text?.get(..10)?;
    let mut parts = date.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    let digits =
        |part: &str, len: usize| part.len() == len && part.bytes().all(|b| b.is_ascii_digit());
    if !(digits(year, 4) && digits(month, 2) && digits(day, 2)) {
        return None;
    }
    let month: usize = month.parse().ok()?;
    let day: u32 = day.parse().ok()?;
    if !(1..=31).contains(&day) {
        return None;
    }
    let name = MONTHS.get(month.checked_sub(1)?)?;
    Some(format!("{day} {name} {year}"))
}

#[cfg(test)]
mod tests {
    use dusk_protocol::plugin::{Kind, Listing};

    use super::{byline, dated_byline, day};

    #[test]
    fn dates_read_in_words_or_not_at_all() {
        assert_eq!(
            day(Some("2026-11-01T10:00:00Z")).as_deref(),
            Some("1 Nov 2026")
        );
        assert_eq!(day(Some("2026-13-01")), None);
        assert_eq!(day(Some("2026-00-01")), None);
        assert_eq!(day(Some("2026-1-011")), None);
        assert_eq!(day(Some("é")), None);
        assert_eq!(day(None), None);
    }

    #[test]
    fn a_byline_leaves_out_what_the_list_does_not_give() {
        let pack = Listing {
            author: "some\none".into(),
            version: "1.2.0".into(),
            kind: Kind::Pack,
            skills: vec!["a".into(), "b".into()],
            updated: Some("2026-11-10T00:00:00Z".into()),
            ..Listing::default()
        };
        assert_eq!(
            dated_byline(&pack),
            "by some one \u{b7} v1.2.0 \u{b7} 2 skills \u{b7} updated 10 Nov 2026"
        );
        assert_eq!(byline(&pack), "by some one \u{b7} v1.2.0 \u{b7} 2 skills");
        let bare = Listing {
            version: "0.1.0".into(),
            ..Listing::default()
        };
        assert_eq!(byline(&bare), "v0.1.0");
    }
}
