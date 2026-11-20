//! What narrows the Workshop's list: the shelf, the search, the tiers and kinds chosen and
//! the category. Every client filters with it, so one search finds the same listings
//! everywhere.

use std::collections::BTreeSet;

use super::{CATEGORIES, Kind, Listing, Order, Scope, Tier};
use crate::catalog::SkillPack;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreFilter<'a> {
    /// The shelf; none shows every kind.
    pub scope: Option<Scope>,
    /// Lower case and trimmed; empty matches everything.
    query: String,
    /// All when empty.
    pub tiers: &'a [Tier],
    /// All when empty.
    pub kinds: &'a [Kind],
    pub category: Option<&'a str>,
}

impl<'a> StoreFilter<'a> {
    pub fn new(
        scope: Option<Scope>,
        query: &str,
        tiers: &'a [Tier],
        kinds: &'a [Kind],
        category: Option<&'a str>,
    ) -> Self {
        Self {
            scope,
            query: query.trim().to_lowercase(),
            tiers,
            kinds,
            category,
        }
    }

    /// Nothing narrows the list and it is in trust order: the storefront, not results.
    pub fn browsing(&self, order: Order) -> bool {
        self.query.is_empty()
            && self.category.is_none()
            && self.tiers.is_empty()
            && self.kinds.is_empty()
            && order == Order::Trust
    }

    /// The listings left after the shelf, tiers, kinds and search, in the given order. A
    /// search looks in the name, the description and the tags.
    pub fn visible<'l>(&self, listings: &'l [Listing]) -> Vec<&'l Listing> {
        listings
            .iter()
            .filter(|listing| self.scope.is_none_or(|scope| scope.holds(listing)))
            .filter(|listing| self.tiers.is_empty() || self.tiers.contains(&listing.tier))
            .filter(|listing| self.kinds.is_empty() || self.kinds.contains(&listing.kind))
            .filter(|listing| {
                let tags = listing.tags.join(" ");
                self.finds(&[&listing.name, &listing.description, &tags])
            })
            .collect()
    }

    /// The packs here that no pack listing of `listings` names, unless a filter leaves
    /// them out: they count as local packs, with no category. A search looks in the name,
    /// the title and the description.
    pub fn local_packs<'p>(
        &self,
        packs: &'p [SkillPack],
        listings: &[Listing],
    ) -> Vec<&'p SkillPack> {
        let shown = self
            .scope
            .is_none_or(|scope| scope.kinds().contains(&Kind::Pack))
            && (self.tiers.is_empty() || self.tiers.contains(&Tier::Local))
            && (self.kinds.is_empty() || self.kinds.contains(&Kind::Pack))
            && self.category.is_none();
        if !shown {
            return Vec::new();
        }
        let listed: BTreeSet<&str> = listings
            .iter()
            .filter(|listing| listing.kind == Kind::Pack)
            .map(|listing| listing.name.as_str())
            .collect();
        packs
            .iter()
            .filter(|pack| !listed.contains(pack.name.as_str()))
            .filter(|pack| self.finds(&[&pack.name, &pack.title, &pack.description]))
            .collect()
    }

    fn finds(&self, texts: &[&str]) -> bool {
        self.query.is_empty()
            || texts
                .iter()
                .any(|text| text.to_lowercase().contains(&self.query))
    }
}

/// The categories the listings name, each once, for a filter menu: the fixed ones in
/// their order, then any other a list gave, by name.
pub fn categories(listings: &[Listing]) -> Vec<String> {
    let named: BTreeSet<&str> = listings
        .iter()
        .flat_map(|listing| {
            listing
                .categories
                .iter()
                .map(String::as_str)
                .chain([listing.category.as_str()])
        })
        .filter(|category| !category.is_empty())
        .collect();
    let fixed = CATEGORIES
        .iter()
        .filter(|category| named.contains(*category));
    let other = named
        .iter()
        .filter(|category| !CATEGORIES.contains(*category));
    fixed
        .chain(other)
        .map(|category| (*category).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(name: &str, kind: Kind, tier: Tier, tags: &[&str]) -> Listing {
        Listing {
            name: name.into(),
            description: format!("about {name}"),
            kind,
            tier,
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            ..Listing::default()
        }
    }

    fn pack(name: &str, title: &str) -> SkillPack {
        SkillPack {
            name: name.into(),
            title: title.into(),
            description: String::new(),
            builtin: false,
            enabled: true,
            skills: Vec::new(),
        }
    }

    #[test]
    fn a_search_finds_tags_and_pack_titles_in_every_client() {
        let listings = [
            listing("lint", Kind::Skill, Tier::Official, &["Rust"]),
            listing("docs", Kind::Pack, Tier::Verified, &[]),
            listing("deploy", Kind::Plugin, Tier::Official, &[]),
        ];
        let packs = [pack("docs", "Docs"), pack("mine", "Rust helpers")];
        let filter = StoreFilter::new(Some(Scope::Skills), "  RUST ", &[], &[], None);
        let names: Vec<&str> = filter
            .visible(&listings)
            .iter()
            .map(|l| l.name.as_str())
            .collect();
        assert_eq!(names, ["lint"]);
        let local: Vec<&str> = filter
            .local_packs(&packs, &listings)
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(
            local,
            ["mine"],
            "a listed pack is not local; the title matches"
        );
        assert!(!filter.browsing(Order::Trust));
        assert!(StoreFilter::default().browsing(Order::Trust));
        assert!(!StoreFilter::default().browsing(Order::Newest));
    }

    #[test]
    fn local_packs_hide_behind_any_filter_that_leaves_them_out() {
        let packs = [pack("mine", "")];
        let tiers = [Tier::Official];
        let kinds = [Kind::Skill];
        for filter in [
            StoreFilter::new(Some(Scope::Plugins), "", &[], &[], None),
            StoreFilter::new(None, "", &tiers, &[], None),
            StoreFilter::new(None, "", &[], &kinds, None),
            StoreFilter::new(None, "", &[], &[], Some("coding")),
        ] {
            assert!(filter.local_packs(&packs, &[]).is_empty(), "{filter:?}");
        }
        let all = StoreFilter::new(None, "", &[Tier::Local], &[Kind::Pack], None);
        assert_eq!(all.local_packs(&packs, &[]).len(), 1);
    }

    #[test]
    fn tiers_and_kinds_keep_only_what_is_chosen() {
        let listings = [
            listing("a", Kind::Skill, Tier::Official, &[]),
            listing("b", Kind::Skill, Tier::Community, &[]),
            listing("c", Kind::Agent, Tier::Official, &[]),
        ];
        let names = |filter: StoreFilter| -> Vec<String> {
            filter
                .visible(&listings)
                .iter()
                .map(|l| l.name.clone())
                .collect()
        };
        assert_eq!(names(StoreFilter::default()), ["a", "b", "c"]);
        let official = [Tier::Official];
        assert_eq!(
            names(StoreFilter::new(None, "", &official, &[], None)),
            ["a", "c"]
        );
        let agents = [Kind::Agent];
        assert_eq!(names(StoreFilter::new(None, "", &[], &agents, None)), ["c"]);
        assert_eq!(
            names(StoreFilter::new(None, "", &official, &[Kind::Skill], None)),
            ["a"]
        );
        assert_eq!(
            names(StoreFilter::new(Some(Scope::Agents), "", &[], &[], None)),
            ["c"]
        );
        assert!(StoreFilter::new(None, "  ", &[], &[], None).browsing(Order::Trust));
    }

    #[test]
    fn categories_list_the_fixed_ones_in_order_then_others_by_name() {
        let filed = |category: &str, more: &[&str]| Listing {
            category: category.into(),
            categories: more.iter().map(|c| c.to_string()).collect(),
            ..Listing::default()
        };
        let listings = [
            filed("security", &["zines", "coding"]),
            filed("", &["art"]),
            filed("coding", &[]),
            filed("", &[""]),
        ];
        assert_eq!(
            categories(&listings),
            ["coding", "security", "art", "zines"]
        );
        assert!(categories(&[]).is_empty());
    }
}
