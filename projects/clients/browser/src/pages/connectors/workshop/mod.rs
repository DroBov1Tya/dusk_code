//! The Workshop in the browser: the same storefront as the desktop app, drawn simpler.
//! Official picks in a carousel, a section per tier, packs with their switches. A
//! browser may look and switch, never install: the server refuses it, so no install
//! button is drawn. The core sorts and filters by category; this only groups by tier.

mod card;
mod pack;

use std::collections::{BTreeSet, HashMap};

use dusk_protocol::{
    catalog::SkillPack,
    plugin::{Kind, Listing, Order, StoreList, Tier},
};
use leptos::prelude::*;

use crate::{
    api,
    ui::{Icon, IconName, run},
};

pub const TIERS: [Tier; 4] = [Tier::Official, Tier::Verified, Tier::Community, Tier::Local];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Facet {
    Category,
    Tier,
    Kind,
}

/// The page's state; every field is a signal, so the struct is copied into handlers.
#[derive(Clone, Copy)]
pub struct Shop {
    list: RwSignal<Option<StoreList>>,
    packs: RwSignal<Vec<SkillPack>>,
    order: RwSignal<Order>,
    category: RwSignal<Option<String>>,
    categories: RwSignal<Vec<String>>,
    facet: RwSignal<Facet>,
    tier: RwSignal<Option<Tier>>,
    kind: RwSignal<Option<Kind>>,
    query: RwSignal<String>,
    pick: RwSignal<usize>,
    /// The pack whose page is open.
    open: RwSignal<Option<String>>,
    /// Covers as `data:` addresses by SHA-256; empty while asked for or when not shown.
    covers: RwSignal<HashMap<String, String>>,
}

impl Shop {
    fn load(self) {
        let (order, category) = (self.order.get_untracked(), self.category.get_untracked());
        let all = category.is_none();
        run(api::store(order, category), move |list| {
            if all {
                self.categories.set(categories(&list.listings));
            }
            self.load_covers(&list.listings);
            self.list.set(Some(list));
        });
        self.load_packs();
    }

    pub fn load_packs(self) {
        run(api::skill_packs(), move |packs| self.packs.set(packs));
    }

    /// Asks once per cover; one that fails keeps the card's letter, with no toast.
    fn load_covers(self, listings: &[Listing]) {
        for listing in listings {
            let Some(cover) = &listing.cover else {
                continue;
            };
            if self
                .covers
                .with_untracked(|covers| covers.contains_key(&cover.sha256))
            {
                continue;
            }
            let sha = cover.sha256.clone();
            self.covers.update(|covers| {
                covers.insert(sha.clone(), String::new());
            });
            let (source, name) = (listing.source.clone(), listing.name.clone());
            leptos::task::spawn_local(async move {
                if let Ok(url) = api::cover(source, name).await {
                    self.covers.update(|covers| {
                        covers.insert(sha, url);
                    });
                }
            });
        }
    }

    pub fn cover(self, listing: &Listing) -> Option<String> {
        let sha = &listing.cover.as_ref()?.sha256;
        self.covers
            .with(|covers| covers.get(sha).filter(|url| !url.is_empty()).cloned())
    }
}

#[component]
pub fn Workshop() -> impl IntoView {
    let shop = Shop {
        list: RwSignal::new(None),
        packs: RwSignal::new(Vec::new()),
        order: RwSignal::new(Order::Trust),
        category: RwSignal::new(None),
        categories: RwSignal::new(Vec::new()),
        facet: RwSignal::new(Facet::Category),
        tier: RwSignal::new(None),
        kind: RwSignal::new(None),
        query: RwSignal::new(String::new()),
        pick: RwSignal::new(0),
        open: RwSignal::new(None),
        covers: RwSignal::new(HashMap::new()),
    };
    shop.load();
    view! {
        <div class="workshop">
            <p class="note">"Look and switch packs here. Installing and updating happen in the \
                desktop app or the terminal: a plugin runs programs on this machine."</p>
            <div class="filters">
                <input type="search" placeholder="Search the Workshop" aria-label="Search the Workshop"
                    prop:value=move || shop.query.get()
                    on:input=move |ev| shop.query.set(event_target_value(&ev)) />
                <button class="ghost" aria-label="Read the store again" on:click=move |_| shop.load()>
                    <Icon name=IconName::RefreshCw />"Refresh"</button>
            </div>
            {move || match shop.open.get() {
                Some(name) => pack::page(shop, name).into_any(),
                None => storefront(shop).into_any(),
            }}
        </div>
    }
}

fn storefront(shop: Shop) -> impl IntoView {
    let Some(list) = shop.list.get() else {
        return view! { <p class="muted">"Reading the store..."</p> }.into_any();
    };
    let shown = visible(shop, &list.listings);
    let browsing = shop.query.with(String::is_empty)
        && shop.category.with(Option::is_none)
        && shop.tier.with(Option::is_none);
    let featured: Vec<Listing> = shown
        .iter()
        .filter(|listing| listing.tier == Tier::Official)
        .take(5)
        .cloned()
        .collect();
    let discover = (browsing && shop.order.get() == Order::Trust && !featured.is_empty())
        .then(|| discover(shop, featured));
    let failed = list
        .failed
        .iter()
        .map(|failure| {
            view! { <p class="note">{format!("{} could not be read: {}", failure.source, failure.error)}</p> }
        })
        .collect_view();
    view! {
        {filters(shop)}
        {failed}
        {discover}
        {sections(shop, &list.listings, shown)}
    }
    .into_any()
}

/// Listings left after the search, tier and kind, in the core's order.
fn visible(shop: Shop, listings: &[Listing]) -> Vec<Listing> {
    let query = shop.query.get().trim().to_lowercase();
    let (tier, kind) = (shop.tier.get(), shop.kind.get());
    listings
        .iter()
        .filter(|listing| tier.is_none_or(|tier| listing.tier == tier))
        .filter(|listing| kind.is_none_or(|kind| listing.kind == kind))
        .filter(|listing| {
            query.is_empty()
                || [&listing.name, &listing.description]
                    .iter()
                    .any(|text| text.to_lowercase().contains(&query))
        })
        .cloned()
        .collect()
}

fn filters(shop: Shop) -> impl IntoView {
    let segment = move |facet: Facet, label: &'static str| {
        view! {
            <button class:active=move || shop.facet.get() == facet
                on:click=move |_| shop.facet.set(facet)>{label}</button>
        }
    };
    let chips = match shop.facet.get() {
        Facet::Category => {
            let current = shop.category.get();
            let mut chips = vec![chip("All".into(), current.is_none(), move || {
                shop.category.set(None);
                shop.load();
            })];
            for category in shop.categories.get() {
                let chosen = current.as_deref() == Some(&category);
                let picked = category.clone();
                chips.push(chip(category, chosen, move || {
                    shop.category.set(Some(picked.clone()));
                    shop.load();
                }));
            }
            chips.collect_view().into_any()
        }
        Facet::Tier => {
            let current = shop.tier.get();
            let mut chips = vec![chip("All".into(), current.is_none(), move || {
                shop.tier.set(None)
            })];
            for each in TIERS {
                chips.push(
                    view! {
                        <button class="chip" class:active=current == Some(each)
                            on:click=move |_| shop.tier.set(Some(each))>{card::mark(each)}</button>
                    }
                    .into_any(),
                );
            }
            chips.collect_view().into_any()
        }
        Facet::Kind => {
            let current = shop.kind.get();
            let mut chips = vec![chip("All".into(), current.is_none(), move || {
                shop.kind.set(None)
            })];
            for each in [Kind::Plugin, Kind::Skill, Kind::Pack, Kind::Agent] {
                chips.push(chip(
                    format!("{}s", card::kind_word(each)),
                    current == Some(each),
                    move || shop.kind.set(Some(each)),
                ));
            }
            chips.collect_view().into_any()
        }
    };
    let orders = [
        (Order::Trust, "Official first"),
        (Order::Newest, "Newest"),
        (Order::Updated, "Recently updated"),
        (Order::Stars, "Most stars"),
    ]
    .map(|(order, label)| {
        chip(label.into(), shop.order.get() == order, move || {
            shop.order.set(order);
            shop.load();
        })
    });
    view! {
        <div class="filters">
            <div class="segmented" role="group" aria-label="Filter by">
                {segment(Facet::Category, "Category")}
                {segment(Facet::Tier, "Tier")}
                {segment(Facet::Kind, "Kind")}
            </div>
            {chips}
        </div>
        <div class="filters" role="group" aria-label="Order">{orders.into_iter().collect_view()}</div>
    }
}

fn chip(label: String, chosen: bool, pick: impl Fn() + 'static) -> AnyView {
    view! { <button class="chip" class:active=chosen on:click=move |_| pick()>{label}</button> }
        .into_any()
}

fn discover(shop: Shop, picks: Vec<Listing>) -> impl IntoView {
    let count = picks.len();
    let index = shop.pick.get() % count;
    let listing = picks[index].clone();
    let cover = shop.cover(&listing);
    let source = match listing.source.as_str() {
        "" => "duskcode".to_owned(),
        source => source.to_owned(),
    };
    let dots = (0..count)
        .map(|dot| {
            view! {
                <button class:active=dot == index aria-label=format!("Pick {}", dot + 1)
                    on:click=move |_| shop.pick.set(dot)></button>
            }
        })
        .collect_view();
    view! {
        <section class="discover" aria-label="Discover">
            <div class="nav">
                <b>"Discover"</b>
                <span>
                    <button class="icon" aria-label="Previous" on:click=move |_| shop.pick.set((index + count - 1) % count)>"\u{2039}"</button>
                    <button class="icon" aria-label="Next" on:click=move |_| shop.pick.set((index + 1) % count)>"\u{203A}"</button>
                </span>
            </div>
            <div class="pick">
                <div class="text">
                    <span class="vendor">{format!("From {source}")}</span>
                    <h3>{listing.name.clone()}</h3>
                    <p class="muted">{listing.description.clone()}</p>
                    <div class="facts">
                        {card::facts(&listing)}
                        <span class="version">{format!("v{}", listing.version)}</span>
                    </div>
                </div>
                {match cover {
                    Some(url) => view! { <img class="cover" src=url alt="" /> }.into_any(),
                    None => view! { <div class="cover" aria-hidden="true"></div> }.into_any(),
                }}
            </div>
            <div class="dots">{dots}</div>
        </section>
    }
}

fn sections(shop: Shop, all: &[Listing], shown: Vec<Listing>) -> impl IntoView {
    let mut parts = Vec::new();
    if shop.order.get() == Order::Trust {
        for each in TIERS {
            let listings: Vec<Listing> = shown.iter().filter(|l| l.tier == each).cloned().collect();
            if !listings.is_empty() {
                parts.push(section(shop, Some(each), card::word(each), listings));
            }
        }
    } else if !shown.is_empty() {
        let title = match shop.order.get() {
            Order::Newest => "Newest",
            Order::Updated => "Recently updated",
            _ => "Most stars",
        };
        parts.push(section(shop, None, title, shown));
    }
    let listed: BTreeSet<&str> = all
        .iter()
        .filter(|listing| listing.kind == Kind::Pack)
        .map(|listing| listing.name.as_str())
        .collect();
    let query = shop.query.get().trim().to_lowercase();
    let local: Vec<SkillPack> = shop
        .packs
        .get()
        .into_iter()
        .filter(|pack| !listed.contains(pack.name.as_str()))
        .filter(|pack| query.is_empty() || pack.name.to_lowercase().contains(&query))
        .collect();
    let local_shown = shop.tier.get().is_none_or(|tier| tier == Tier::Local)
        && shop.kind.get().is_none_or(|kind| kind == Kind::Pack)
        && shop.category.get().is_none();
    if local_shown && !local.is_empty() {
        parts.push(
            view! {
                <section>
                    <h3 class="shelf-head"><span class="tier tier-local"><Icon name=IconName::TierLocal /></span>
                        "Built-in and local packs"</h3>
                    <div class="cards">{local.into_iter().map(|pack| card::pack(shop, pack)).collect_view()}</div>
                </section>
            }
            .into_any(),
        );
    }
    if parts.is_empty() {
        return view! { <p class="muted">"Nothing matches."</p> }.into_any();
    }
    parts.collect_view().into_any()
}

fn section(shop: Shop, tier: Option<Tier>, title: &str, listings: Vec<Listing>) -> AnyView {
    let count = listings.len();
    let icon = tier.map(|tier| {
        view! { <span class=format!("tier {}", card::class(tier))><Icon name=card::icon(tier) /></span> }
    });
    view! {
        <section>
            <h3 class="shelf-head">{icon}{title.to_owned()}<span class="muted">{count}</span></h3>
            <div class="cards">{listings.into_iter().map(|listing| card::listing(shop, listing)).collect_view()}</div>
        </section>
    }
    .into_any()
}

fn categories(listings: &[Listing]) -> Vec<String> {
    let set: BTreeSet<&str> = listings
        .iter()
        .map(|listing| listing.category.as_str())
        .filter(|category| !category.is_empty())
        .collect();
    set.into_iter().map(str::to_owned).collect()
}
