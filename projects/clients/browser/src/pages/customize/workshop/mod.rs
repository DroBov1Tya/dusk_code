//! The Workshop in the browser: the same storefront as the desktop app, on each shelf of
//! Customize with the listings of its `Scope`. A featured hero
//! of official picks, shelves of cards (a tier each, the core's picks of the newest and
//! the latest updated, the local packs), one Filters menu, a
//! listing's window with its gallery, and a pack's page with its switches. A browser may
//! look and switch, never install: the server refuses it, so no install button is drawn.
//! The core sorts, filters by category and picks the shelves; tiers and kinds only
//! narrow what is shown here. The hero, the shelves and the results share one grid of 1 to
//! 4 columns from the page's measured width; a shelf shows one row and pages by a row.

mod card;
mod detail;
mod filters;
mod hero;
mod pack;
mod placeholder;
mod suggested;

pub use suggested::SuggestedCard;

use std::collections::{BTreeSet, HashMap};

use dusk_protocol::{
    catalog::SkillPack,
    plugin::{Kind, Listing, Order, Scope, StoreList, Tier},
};
use leptos::{html, prelude::*};
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::ResizeObserver;

use crate::{
    api,
    ui::{Icon, IconName, run},
};
use placeholder::Picture;

pub const TIERS: [Tier; 5] = [
    Tier::Official,
    Tier::Verified,
    Tier::Community,
    Tier::Unverified,
    Tier::Local,
];

/// The grid, as the desktop's: cards at least this wide, gaps of 16, at most 4 columns
/// (the page stops at 1184 px, where a fifth never fits).
const CARD_MIN: usize = 232;
const GAP: usize = 16;
const COLUMNS_MAX: usize = 4;
/// Under this width the hero's cover goes on top of its text.
const NARROW: i32 = 744;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shelf {
    Tier(Tier),
    Newest,
    Updated,
    Local,
}

const SHELVES: [Shelf; 6] = [
    Shelf::Tier(Tier::Official),
    Shelf::Tier(Tier::Verified),
    Shelf::Tier(Tier::Community),
    Shelf::Newest,
    Shelf::Updated,
    Shelf::Local,
];

impl Shelf {
    fn title(self) -> &'static str {
        match self {
            Self::Tier(tier) => card::word(tier),
            Self::Newest => "New",
            Self::Updated => "Recently updated",
            Self::Local => "Built-in and local",
        }
    }
}

/// The page's state; every field is a signal, so the struct is copied into handlers.
#[derive(Clone, Copy)]
pub struct Shop {
    /// The shelf of Customize this Workshop is on, which says what it lists.
    scope: Scope,
    list: RwSignal<Option<StoreList>>,
    packs: RwSignal<Vec<SkillPack>>,
    order: RwSignal<Order>,
    category: RwSignal<Option<String>>,
    categories: RwSignal<Vec<String>>,
    tiers: RwSignal<Vec<Tier>>,
    kinds: RwSignal<Vec<Kind>>,
    query: RwSignal<String>,
    menu: RwSignal<bool>,
    hero: RwSignal<usize>,
    /// The shelf shown whole after "See all".
    shelf: RwSignal<Option<Shelf>>,
    /// The pack whose page is open.
    open: RwSignal<Option<String>>,
    /// The listing whose window is open, by source and name, and its picture in view.
    window: RwSignal<Option<(String, String)>>,
    gallery: RwSignal<usize>,
    /// Covers by SHA-256, from asked for to shown or missing.
    covers: RwSignal<HashMap<String, Picture>>,
    /// Screenshots by `source/name@version`, each empty while asked for or when not shown.
    shots: RwSignal<HashMap<String, Vec<String>>>,
    /// The page's measured width in CSS pixels, and what follows from it.
    width: RwSignal<i32>,
    columns: Memo<usize>,
    narrow: Memo<bool>,
}

impl Shop {
    fn load(self) {
        let (order, category) = (self.order.get_untracked(), self.category.get_untracked());
        let all = category.is_none();
        run(
            api::store(order, category, self.scope.kinds()),
            move |list| {
                if all {
                    self.categories.set(categories(&list.listings));
                }
                self.load_covers(&list.listings);
                self.list.set(Some(list));
            },
        );
        self.load_packs();
    }

    pub fn load_packs(self) {
        run(api::skill_packs(), move |packs| self.packs.set(packs));
    }

    /// Asks once per cover, and again on a later read for one that failed. A failure
    /// draws the placeholder (offline when this browser has no network); its reason goes
    /// to the console, not to the card.
    fn load_covers(self, listings: &[Listing]) {
        for listing in listings {
            let Some(cover) = &listing.cover else {
                continue;
            };
            let asked = self.covers.with_untracked(|covers| {
                matches!(
                    covers.get(&cover.sha256),
                    Some(Picture::Loading | Picture::Shown(_))
                )
            });
            if asked {
                continue;
            }
            let sha = cover.sha256.clone();
            self.covers.update(|covers| {
                covers.insert(sha.clone(), Picture::Loading);
            });
            let (source, name) = (listing.source.clone(), listing.name.clone());
            leptos::task::spawn_local(async move {
                let picture = match api::cover(source, name.clone()).await {
                    Ok(url) => Picture::Shown(url),
                    Err(error) => {
                        web_sys::console::warn_1(&format!("cover of {name}: {error}").into());
                        if offline() {
                            Picture::Offline
                        } else {
                            Picture::Missing
                        }
                    }
                };
                self.covers.update(|covers| {
                    covers.insert(sha, picture);
                });
            });
        }
    }

    /// Asks once per version for a listing's screenshots, when its window opens.
    fn load_shots(self, listing: &Listing) {
        let key = shots_key(listing);
        let count = listing.screenshots.len();
        if count == 0 || self.shots.with_untracked(|shots| shots.contains_key(&key)) {
            return;
        }
        self.shots.update(|shots| {
            shots.insert(key.clone(), vec![String::new(); count]);
        });
        for index in 0..count {
            let (source, name, key) = (listing.source.clone(), listing.name.clone(), key.clone());
            leptos::task::spawn_local(async move {
                if let Ok(url) = api::screenshot(source, name, index as u32).await {
                    self.shots.update(|shots| {
                        if let Some(slot) = shots.get_mut(&key).and_then(|all| all.get_mut(index)) {
                            *slot = url;
                        }
                    });
                }
            });
        }
    }

    /// A cover by its SHA-256; a listing without one has none to wait for.
    pub fn picture(self, sha: Option<&str>) -> Picture {
        let Some(sha) = sha else {
            return Picture::Missing;
        };
        self.covers
            .with(|covers| covers.get(sha).cloned().unwrap_or(Picture::Loading))
    }

    fn open_listing(self, listing: &Listing) {
        self.menu.set(false);
        self.gallery.set(0);
        self.load_shots(listing);
        self.window
            .set(Some((listing.source.clone(), listing.name.clone())));
    }

    fn find(self, source: &str, name: &str) -> Option<Listing> {
        self.list.with(|list| {
            list.as_ref()?
                .listings
                .iter()
                .find(|listing| listing.source == source && listing.name == name)
                .cloned()
        })
    }

    /// Nothing narrows the list and it is in trust order: the storefront, not results.
    fn browsing(self) -> bool {
        self.query.with(|query| query.trim().is_empty())
            && self.category.with(Option::is_none)
            && self.tiers.with(Vec::is_empty)
            && self.kinds.with(Vec::is_empty)
            && self.order.get() == Order::Trust
    }
}

fn offline() -> bool {
    web_sys::window().is_some_and(|window| !window.navigator().on_line())
}

/// The grid's columns for a page this wide.
fn columns(width: i32) -> usize {
    let width = usize::try_from(width).unwrap_or(0);
    ((width + GAP) / (CARD_MIN + GAP)).clamp(1, COLUMNS_MAX)
}

/// Keeps `width` at the element's width while it is shown, so the grid follows the room
/// it has (a panel opening beside it, too), not the window's.
fn watch_width(node: NodeRef<html::Div>, width: RwSignal<i32>) {
    let watcher = StoredValue::new_local(None::<(ResizeObserver, Closure<dyn FnMut()>)>);
    Effect::new(move || {
        let Some(element) = node.get() else {
            return;
        };
        if watcher.with_value(Option::is_some) {
            return;
        }
        width.set(element.client_width());
        let measured = element.clone();
        let resized = Closure::<dyn FnMut()>::new(move || width.set(measured.client_width()));
        match ResizeObserver::new(resized.as_ref().unchecked_ref()) {
            Ok(observer) => {
                observer.observe(&element);
                watcher.set_value(Some((observer, resized)));
            }
            Err(error) => web_sys::console::warn_1(&error),
        }
    });
    on_cleanup(move || {
        if let Some((observer, _)) = watcher.try_update_value(Option::take).flatten() {
            observer.disconnect();
        }
    });
}

fn shots_key(listing: &Listing) -> String {
    format!("{}/{}@{}", listing.source, listing.name, listing.version)
}

/// "Workshop  Discover | Installed" of a tab of Customize.
#[component]
pub fn ViewSwitch(discover: RwSignal<bool>) -> impl IntoView {
    view! {
        <div class="shelf-head" role="group" aria-label="Workshop">
            <h3>"Workshop"</h3>
            <button class="tab" class:active=move || discover.get()
                on:click=move |_| discover.set(true)>"Discover"</button>
            <button class="tab" class:active=move || !discover.get()
                on:click=move |_| discover.set(false)>"Installed"</button>
        </div>
    }
}

/// An empty Installed list, with the way to the tab's Discover.
pub fn nothing_installed(scope: Scope, discover: RwSignal<bool>) -> impl IntoView {
    view! {
        <div class="row-actions">
            <p class="muted">{format!("Nothing installed yet. Discover {} in the Workshop.", scope.noun())}</p>
            <button class="ghost" on:click=move |_| discover.set(true)>"Discover"</button>
        </div>
    }
}

#[component]
pub fn Workshop(scope: Scope) -> impl IntoView {
    let width = RwSignal::new(0);
    let shop = Shop {
        scope,
        list: RwSignal::new(None),
        packs: RwSignal::new(Vec::new()),
        order: RwSignal::new(Order::Trust),
        category: RwSignal::new(None),
        categories: RwSignal::new(Vec::new()),
        tiers: RwSignal::new(Vec::new()),
        kinds: RwSignal::new(Vec::new()),
        query: RwSignal::new(String::new()),
        menu: RwSignal::new(false),
        hero: RwSignal::new(0),
        shelf: RwSignal::new(None),
        open: RwSignal::new(None),
        window: RwSignal::new(None),
        gallery: RwSignal::new(0),
        covers: RwSignal::new(HashMap::new()),
        shots: RwSignal::new(HashMap::new()),
        width,
        columns: Memo::new(move |_| columns(width.get())),
        narrow: Memo::new(move |_| width.get() < NARROW),
    };
    shop.load();
    let node = NodeRef::<html::Div>::new();
    watch_width(node, shop.width);
    view! {
        <div node_ref=node class=move || {
            let narrow = if shop.narrow.get() { " narrow" } else { "" };
            format!("workshop cols-{}{narrow}", shop.columns.get())
        }>
            <p class="note">"Look and switch packs here. Installing and updating happen in the \
                desktop app or the terminal: a plugin runs programs on this machine."</p>
            <div class="store-bar">
                <input type="search" placeholder="Search the Workshop" aria-label="Search the Workshop"
                    prop:value=move || shop.query.get()
                    on:input=move |ev| shop.query.set(event_target_value(&ev)) />
                {filters::button(shop)}
                <button class="icon" title="Read the store again" aria-label="Read the store again"
                    on:click=move |_| shop.load()><Icon name=IconName::RefreshCw /></button>
            </div>
            {move || filters::chips(shop)}
            {move || match shop.open.get() {
                Some(name) => pack::page(shop, name).into_any(),
                None => storefront(shop).into_any(),
            }}
            {move || shop.window.get().map(|(source, name)| detail::window(shop, source, name))}
        </div>
    }
}

fn storefront(shop: Shop) -> impl IntoView {
    let Some(list) = shop.list.get() else {
        return view! { <p class="muted">"Reading the store..."</p> }.into_any();
    };
    let shown = visible(shop, &list.listings);
    let failed = list
        .failed
        .iter()
        .map(|failure| {
            view! { <p class="note">{format!("{} could not be read: {}", failure.source, failure.error)}</p> }
        })
        .collect_view();
    let body = if !shop.browsing() {
        let mut items: Vec<Item> = shown
            .into_iter()
            .map(|listing| Item::Listing(Box::new(listing)))
            .collect();
        items.extend(
            local_packs(shop, &list.listings)
                .into_iter()
                .map(Item::Pack),
        );
        results(shop, items)
    } else if let Some(shelf) = shop.shelf.get() {
        let items = items(shop, shelf, &list, &shown);
        view! {
            <section class="shelf">
                <div><button class="ghost" on:click=move |_| shop.shelf.set(None)>
                    <Icon name=IconName::ChevronLeft />"All shelves"</button></div>
                {heading(Some(shelf), shelf.title(), items.len(), None)}
                <div class="capsules">{capsules(shop, items)}</div>
            </section>
        }
        .into_any()
    } else {
        let picks: Vec<Listing> = shown
            .iter()
            .filter(|listing| listing.tier == Tier::Official)
            .take(5)
            .cloned()
            .collect();
        let shelves = SHELVES
            .into_iter()
            .filter_map(|shelf| {
                let items = items(shop, shelf, &list, &shown);
                (!items.is_empty()).then(|| shelf_row(shop, shelf, items))
            })
            .collect_view();
        view! {
            {(!picks.is_empty()).then(|| hero::hero(shop, picks))}
            {shelves}
        }
        .into_any()
    };
    view! { {failed} {body} }.into_any()
}

/// What a shelf holds.
#[derive(Clone)]
enum Item {
    Listing(Box<Listing>),
    Pack(SkillPack),
}

fn items(shop: Shop, shelf: Shelf, list: &StoreList, shown: &[Listing]) -> Vec<Item> {
    let picked = |indices: &[usize]| -> Vec<Item> {
        indices
            .iter()
            .filter_map(|&index| list.listings.get(index))
            .filter(|listing| shop.scope.holds(listing))
            .cloned()
            .map(|listing| Item::Listing(Box::new(listing)))
            .collect()
    };
    match shelf {
        Shelf::Tier(tier) => shown
            .iter()
            .filter(|listing| listing.tier == tier)
            .cloned()
            .map(|listing| Item::Listing(Box::new(listing)))
            .collect(),
        Shelf::Newest => picked(&list.picks.newest),
        Shelf::Updated => picked(&list.picks.updated),
        Shelf::Local => shown
            .iter()
            .filter(|listing| listing.tier == Tier::Local)
            .cloned()
            .map(|listing| Item::Listing(Box::new(listing)))
            .chain(
                local_packs(shop, &list.listings)
                    .into_iter()
                    .map(Item::Pack),
            )
            .collect(),
    }
}

fn capsules(shop: Shop, items: Vec<Item>) -> impl IntoView {
    items
        .into_iter()
        .map(|item| match item {
            Item::Listing(listing) => card::capsule(shop, *listing),
            Item::Pack(pack) => card::pack(shop, pack).into_any(),
        })
        .collect_view()
}

/// One shelf: one row of the grid, paged by a row with its arrows, and "See all" when it
/// holds more than a row.
fn shelf_row(shop: Shop, shelf: Shelf, items: Vec<Item>) -> AnyView {
    let count = items.len();
    let items = StoredValue::new(items);
    let page = RwSignal::new(0usize);
    let pages = move || count.div_ceil(shop.columns.get()).max(1);
    let shown = move || page.get().min(pages() - 1);
    let title = shelf.title();
    // `aria-disabled`, not `disabled`: a button that disables itself under the keyboard
    // would drop the focus to the page.
    let first = move || (shown() == 0).to_string();
    let last = move || (shown() + 1 >= pages()).to_string();
    let end = move || {
        (count > shop.columns.get()).then(|| view! {
            <button class="icon" aria-label=format!("Previous row of {title}") aria-disabled=first
                on:click=move |_| page.set(shown().saturating_sub(1))><Icon name=IconName::ChevronLeft /></button>
            <button class="icon" aria-label=format!("Next row of {title}") aria-disabled=last
                on:click=move |_| page.set((shown() + 1).min(pages() - 1))><Icon name=IconName::ChevronRight /></button>
            <button class="ghost quiet" on:click=move |_| shop.shelf.set(Some(shelf))>"See all"</button>
        })
    };
    let row = move || {
        let columns = shop.columns.get();
        let start = shown() * columns;
        let items =
            items.with_value(|items| items.iter().skip(start).take(columns).cloned().collect());
        capsules(shop, items)
    };
    view! {
        <section class="shelf">
            {heading(Some(shelf), title, count, Some(view! { {end} }.into_any()))}
            <div class="capsules">{row}</div>
        </section>
    }
    .into_any()
}

fn results(shop: Shop, items: Vec<Item>) -> AnyView {
    if items.is_empty() {
        return view! { <p class="muted">"Nothing matches. Try another word or filter."</p> }
            .into_any();
    }
    view! {
        <section class="shelf">
            {heading(None, "Results", items.len(), None)}
            <div class="capsules">{capsules(shop, items)}</div>
        </section>
    }
    .into_any()
}

fn heading(shelf: Option<Shelf>, title: &str, count: usize, end: Option<AnyView>) -> impl IntoView {
    let icon = shelf.map(|shelf| match shelf {
        Shelf::Tier(tier) => view! { <span class=format!("tier {}", card::class(tier))><Icon name=card::icon(tier) /></span> }.into_any(),
        Shelf::Local => view! { <span class="tier tier-local"><Icon name=IconName::TierLocal /></span> }.into_any(),
        Shelf::Newest => view! { <Icon name=IconName::Calendar /> }.into_any(),
        Shelf::Updated => view! { <Icon name=IconName::RefreshCw /> }.into_any(),
    });
    view! {
        <div class="shelf-head">
            {icon}<h3>{title.to_owned()}</h3><span class="muted">{count}</span>
            <span class="grow"></span>{end}
        </div>
    }
}

/// The shelf's listings left after the search, tiers and kinds, in the core's order.
fn visible(shop: Shop, listings: &[Listing]) -> Vec<Listing> {
    let query = shop.query.get().trim().to_lowercase();
    let (tiers, kinds) = (shop.tiers.get(), shop.kinds.get());
    listings
        .iter()
        .filter(|listing| shop.scope.holds(listing))
        .filter(|listing| tiers.is_empty() || tiers.contains(&listing.tier))
        .filter(|listing| kinds.is_empty() || kinds.contains(&listing.kind))
        .filter(|listing| {
            query.is_empty()
                || [&listing.name, &listing.description, &listing.tags.join(" ")]
                    .iter()
                    .any(|text| text.to_lowercase().contains(&query))
        })
        .cloned()
        .collect()
}

/// Packs here that no listing names, unless a filter leaves them out.
fn local_packs(shop: Shop, listings: &[Listing]) -> Vec<SkillPack> {
    let listed: BTreeSet<&str> = listings
        .iter()
        .filter(|listing| listing.kind == Kind::Pack)
        .map(|listing| listing.name.as_str())
        .collect();
    let query = shop.query.get().trim().to_lowercase();
    let (tiers, kinds) = (shop.tiers.get(), shop.kinds.get());
    let shown = shop.scope.kinds().contains(&Kind::Pack)
        && (tiers.is_empty() || tiers.contains(&Tier::Local))
        && (kinds.is_empty() || kinds.contains(&Kind::Pack))
        && shop.category.get().is_none();
    if !shown {
        return Vec::new();
    }
    shop.packs
        .get()
        .into_iter()
        .filter(|pack| !listed.contains(pack.name.as_str()))
        .filter(|pack| query.is_empty() || pack.name.to_lowercase().contains(&query))
        .collect()
}

fn categories(listings: &[Listing]) -> Vec<String> {
    let set: BTreeSet<&str> = listings
        .iter()
        .map(|listing| listing.category.as_str())
        .filter(|category| !category.is_empty())
        .collect();
    set.into_iter().map(str::to_owned).collect()
}
