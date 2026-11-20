//! The Workshop: the store's listings and the skill packs here, narrowed by what is
//! typed, a pack's page with its switches, and the box that asks before a listing is
//! installed, updated or removed. The core sorts and filters by category; this keeps
//! what it read and what the user chose.

use dusk_protocol::{
    catalog::SkillPack,
    plugin::{Kind, Listing, Order, Review, SourceFailure, Tier},
};

#[derive(Default)]
pub struct Workshop {
    /// `None` until the core has read the store.
    pub listings: Option<Vec<Listing>>,
    /// The sources the last read could not reach.
    pub failed: Vec<SourceFailure>,
    /// The skill packs here, with their switches.
    pub packs: Option<Vec<SkillPack>>,
    pub order: Order,
    /// The one category the core lists, when one is chosen.
    pub category: Option<String>,
    /// The categories of the last list read without one; Tab steps through them.
    pub categories: Vec<String>,
    /// Typed text; rows whose name or description hold it stay.
    pub filter: String,
    /// Among the rows `shown` returns.
    pub selected: usize,
    /// The pack whose page is open.
    pub pack: Option<PackPage>,
    pub confirm: Option<Confirm>,
    /// What the last load, install, update or removal is doing or did.
    pub step: Option<Step>,
}

pub struct PackPage {
    pub name: String,
    /// Row 0 is the pack's own switch, then its skills.
    pub selected: usize,
}

pub enum Step {
    Working(String),
    Done(String),
    Failed(String),
}

/// The question about one listing, with the button in focus.
pub struct Confirm {
    pub listing: Listing,
    pub focus: usize,
    /// For an Unverified listing: the files an install would write, read before the yes.
    pub review: Option<Reviewed>,
}

/// What the core read of an Unverified listing's files, or why it could not.
pub enum Reviewed {
    Reading,
    Read(Box<Review>),
    Failed(String),
}

impl Confirm {
    /// The hash an install names: the files shown, when they were read and may be
    /// installed. `None` for any other tier, which needs none.
    pub fn agreed(&self) -> Option<Option<String>> {
        if !reviewed_first(&self.listing) {
            return Some(None);
        }
        match &self.review {
            Some(Reviewed::Read(review)) if review.refused.is_none() => {
                Some(Some(review.package_sha256.clone()))
            }
            _ => None,
        }
    }
}

/// Whether the files of `listing` are read and shown before the yes: an Unverified
/// one's, since nobody signed its list. A connector has no files; its address is in the
/// yes itself.
pub fn reviewed_first(listing: &Listing) -> bool {
    listing.tier == Tier::Unverified && listing.kind != dusk_protocol::plugin::Kind::Connector
}

/// A row of the list: a listing of the store, or a pack here that no source lists.
#[derive(Clone, Copy)]
pub enum Row<'a> {
    Listing(&'a Listing),
    Pack(&'a SkillPack),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Install,
    Update,
    Remove,
    Cancel,
}

impl Choice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Install => "Install",
            Self::Update => "Update",
            Self::Remove => "Remove",
            Self::Cancel => "Cancel",
        }
    }
}

/// What the box offers for `listing`: installing only where it fits, updating only when
/// the source has a newer version, removing only what the store installed, Cancel last.
pub fn choices(listing: &Listing) -> Vec<Choice> {
    let mut choices = Vec::new();
    if listing.unfit.is_none() {
        match (&listing.installed, &listing.update) {
            (None, _) => choices.push(Choice::Install),
            (Some(_), Some(_)) => choices.push(Choice::Update),
            (Some(_), None) => {}
        }
    }
    if listing.installed.is_some() {
        choices.push(Choice::Remove);
    }
    choices.push(Choice::Cancel);
    choices
}

/// The order's name, as the list's header says it.
pub fn order_word(order: Order) -> &'static str {
    match order {
        Order::Trust => "official first",
        Order::Newest => "newest",
        Order::Updated => "recently updated",
    }
}

impl Workshop {
    /// The listings that match the filter in the store's order, then the packs here that
    /// no listing names.
    pub fn shown(&self) -> Vec<Row<'_>> {
        let filter = self.filter.to_lowercase();
        let hit = |texts: &[&str]| {
            filter.is_empty()
                || texts
                    .iter()
                    .any(|text| text.to_lowercase().contains(&filter))
        };
        let listings = self.listings.iter().flatten();
        let mut rows: Vec<Row<'_>> = listings
            .clone()
            .filter(|listing| hit(&[&listing.name, &listing.description]))
            .map(Row::Listing)
            .collect();
        if self.category.is_none() {
            let listed = |name: &str| {
                listings
                    .clone()
                    .any(|listing| listing.kind == Kind::Pack && listing.name == name)
            };
            rows.extend(
                self.packs
                    .iter()
                    .flatten()
                    .filter(|pack| !listed(&pack.name))
                    .filter(|pack| hit(&[&pack.name, &pack.title, &pack.description]))
                    .map(Row::Pack),
            );
        }
        rows
    }

    /// The listing of a pack by name, when a source lists it.
    pub fn pack_listing(&self, name: &str) -> Option<&Listing> {
        self.listings
            .iter()
            .flatten()
            .find(|listing| listing.kind == Kind::Pack && listing.name == name)
    }

    /// The pack here by name, with its switches.
    pub fn pack_here(&self, name: &str) -> Option<&SkillPack> {
        self.packs.iter().flatten().find(|pack| pack.name == name)
    }

    /// The next order, round.
    pub fn next_order(&mut self) {
        self.order = match self.order {
            Order::Trust => Order::Newest,
            Order::Newest => Order::Updated,
            Order::Updated => Order::Trust,
        };
        self.selected = 0;
    }

    /// The next category, then all of them again.
    pub fn next_category(&mut self) {
        let at = self
            .category
            .as_ref()
            .and_then(|current| self.categories.iter().position(|each| each == current));
        self.category = match at {
            None => self.categories.first().cloned(),
            Some(index) => self.categories.get(index + 1).cloned(),
        };
        self.selected = 0;
    }
}
