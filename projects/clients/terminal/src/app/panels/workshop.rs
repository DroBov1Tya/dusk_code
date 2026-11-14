//! The Workshop: the plugin store's list, narrowed by what is typed, and the box that asks
//! before a plugin is installed, updated or removed.

use dusk_protocol::plugin::Listing;

#[derive(Default)]
pub struct Workshop {
    /// `None` until the core has read the store.
    pub listings: Option<Vec<Listing>>,
    /// Typed text; rows whose name or description hold it stay.
    pub filter: String,
    /// Among the rows `shown` returns.
    pub selected: usize,
    pub confirm: Option<Confirm>,
    /// What the last load, install or removal is doing or did.
    pub step: Option<Step>,
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

/// What the box offers for `listing`: installing only where it fits, removing only what
/// the store installed, Cancel always last.
pub fn choices(listing: &Listing) -> Vec<Choice> {
    let mut choices = Vec::new();
    if listing.unfit.is_none() {
        match &listing.installed {
            None => choices.push(Choice::Install),
            Some(version) if *version != listing.version => choices.push(Choice::Update),
            Some(_) => {}
        }
    }
    if listing.installed.is_some() {
        choices.push(Choice::Remove);
    }
    choices.push(Choice::Cancel);
    choices
}

impl Workshop {
    /// The listings that match the filter, in the store's order.
    pub fn shown(&self) -> Vec<&Listing> {
        let filter = self.filter.to_lowercase();
        self.listings
            .iter()
            .flatten()
            .filter(|listing| {
                filter.is_empty()
                    || listing.name.to_lowercase().contains(&filter)
                    || listing.description.to_lowercase().contains(&filter)
            })
            .collect()
    }
}
