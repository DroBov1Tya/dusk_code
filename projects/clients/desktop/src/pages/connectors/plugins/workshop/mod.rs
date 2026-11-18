//! The Workshop: what the store's sources offer, read through the core, as a storefront.
//! A featured carousel of official picks, then shelves of capsules (a tier each, the
//! newest, the latest updated, the most starred, the local packs); a capsule opens the
//! listing's window, a pack also has a page of its skills and their switches. The core
//! checks, sorts, filters by category and picks the shelves; this page only shows what
//! it read, narrows by tier and kind, and asks before anything changes.

mod actions;
mod capsule;
pub mod covers;
pub mod detail;
pub mod filters;
mod gallery;
mod grid;
mod hero;
mod pack;
mod placeholder;
mod shelves;
pub mod tier;

use std::{collections::HashMap, time::Instant};

use dusk_protocol::{
    catalog::SkillPack,
    client::Switch as CoreSwitch,
    plugin::{Kind, Listing, Order, Picks, SourceFailure, Tier, UpdateReport},
};
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::Confirm;
use crate::{
    modal::Modal,
    ui::{Type, size, widgets},
    workspace::Workspace,
};

pub use grid::Grid;
pub use shelves::Shelf;

#[derive(Default)]
pub struct Workshop {
    /// The Workshop is shown on the plugins shelf instead of the installed plugins.
    pub open: bool,
    /// The store's listings, once read; kept while it is read again.
    pub listings: Option<Vec<Listing>>,
    /// The core's picks for the New, Recently updated and Top rated shelves.
    pub picks: Picks,
    /// The sources the last read could not reach; the others are listed all the same.
    pub failed_sources: Vec<SourceFailure>,
    pub loading: bool,
    /// Why the store could not be asked at all.
    pub error: Option<String>,
    /// Listings being installed or removed, by name, with what the card says meanwhile.
    pub busy: HashMap<String, &'static str>,
    /// Why the last install or removal of a listing failed, by name.
    pub failed: HashMap<String, String>,
    /// The order the core sorts the list in.
    pub order: Order,
    /// The one category the core lists, when one is chosen.
    pub category: Option<String>,
    /// The categories of the last list read without one, for the filter menu.
    pub categories: Vec<String>,
    /// The tiers and kinds shown; all of them when empty.
    pub tiers: Vec<Tier>,
    pub kinds: Vec<Kind>,
    /// The filter menu is open; when it last closed, so the click that closed it does
    /// not open it again.
    pub filters: bool,
    pub filters_closed: Option<Instant>,
    /// The featured pick in view.
    pub hero: usize,
    /// The shelf shown whole after "See all".
    pub shelf: Option<Shelf>,
    /// The first capsule each shelf shows, after its arrows.
    pub scrolled: HashMap<Shelf, usize>,
    /// The skill packs here, with their switches.
    pub packs: Option<Vec<SkillPack>>,
    /// The pack whose page is open, by name.
    pub pack: Option<String>,
    pub covers: covers::Covers,
    pub shots: covers::Shots,
    /// The gallery of the open listing's window: the picture in view and its strip.
    pub gallery: usize,
    pub strip: ScrollHandle,
    /// What the last "Update all" did, until it is closed.
    pub report: Option<UpdateReport>,
    pub updating: bool,
    /// The page's width as last drawn, which the grid's columns come from.
    pub measured: grid::Measured,
}

pub fn render(ws: &Workspace, query: &str, window: &Window, cx: &mut Context<Workspace>) -> Div {
    let shop = &ws.workshop;
    let Some(listings) = &shop.listings else {
        return match &shop.error {
            Some(error) if !shop.loading => unreachable_store(error, cx),
            _ => crate::ui::loading("Reading the store...", cx),
        };
    };
    let grid = Grid::of(ws, window);
    // The page fills the column so its width can be measured; the grid stays inside it.
    let measured = div()
        .relative()
        .w_full()
        .child(grid::measure(&shop.measured));
    if let Some(name) = &shop.pack {
        return measured.child(pack::render(ws, name, grid, cx));
    }
    let mut page = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_XXL))
        .w(px(grid.width));
    let mut notes = div().flex().flex_col().gap(px(size::GAP_M));
    if let Some(chips) = filters::chips(ws, cx) {
        notes = notes.child(chips);
    }
    if let Some(error) = shop.error.as_ref().filter(|_| !shop.loading) {
        notes = notes.child(widgets::muted(
            format!("The store could not be read again: {error}"),
            cx,
        ));
    }
    if let Some(report) = &shop.report {
        notes = notes.child(update_report(report, cx));
    }
    if !shop.failed_sources.is_empty() {
        notes = notes.child(failed_sources(&shop.failed_sources, cx));
    }
    page = page.child(notes);
    let shown = visible(shop, listings, query);
    if browsing(shop, query) && shop.shelf.is_none() {
        let featured = hero::featured(&shown);
        if !featured.is_empty() {
            page = page.child(hero::render(ws, &featured, grid, cx));
        }
    }
    measured.child(page.child(shelves::render(ws, &shown, query, grid, cx)))
}

/// Nothing narrows the list and it is in trust order: the storefront, not results.
fn browsing(shop: &Workshop, query: &str) -> bool {
    query.is_empty()
        && shop.category.is_none()
        && shop.tiers.is_empty()
        && shop.kinds.is_empty()
        && shop.order == Order::Trust
}

/// The listings left after the search, the tiers and the kinds chosen, in the core's order.
fn visible<'a>(shop: &Workshop, listings: &'a [Listing], query: &str) -> Vec<&'a Listing> {
    listings
        .iter()
        .filter(|listing| shop.tiers.is_empty() || shop.tiers.contains(&listing.tier))
        .filter(|listing| shop.kinds.is_empty() || shop.kinds.contains(&listing.kind))
        .filter(|listing| {
            let tags = listing.tags.join(" ");
            super::super::matches(query, &[&listing.name, &listing.description, &tags])
        })
        .collect()
}

/// The listing a window or a page is about, by its source and name.
pub fn find<'a>(ws: &'a Workspace, source: &str, name: &str) -> Option<&'a Listing> {
    ws.workshop
        .listings
        .iter()
        .flatten()
        .find(|listing| listing.source == source && listing.name == name)
}

/// A calm line for each source that could not be read.
fn failed_sources(failures: &[SourceFailure], cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_col()
        .gap_1()
        .children(failures.iter().map(|failure| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .meta()
                .text_color(muted)
                .child(widgets::icon(IconName::CircleAlert, muted))
                .child(format!(
                    "{} could not be read: {}",
                    failure.source, failure.error
                ))
        }))
}

/// What "Update all" updated, and what it left for the user with why.
fn update_report(report: &UpdateReport, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, success, warning) = (theme.muted_foreground, theme.success, theme.warning);
    let headline = match report.updated.len() {
        0 => "Nothing was updated".to_owned(),
        n => format!("Updated {}", widgets::count(n, "listing")),
    };
    let mut body = div().flex().flex_col().gap_1().flex_1().min_w_0().child(
        div()
            .body()
            .font_weight(FontWeight::SEMIBOLD)
            .child(headline),
    );
    for line in &report.updated {
        body = body.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .small()
                .child(widgets::icon(IconName::Check, success))
                .child(line.clone()),
        );
    }
    for (name, why) in &report.skipped {
        body = body.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .small()
                .child(widgets::icon(IconName::CircleAlert, warning))
                .child(format!("{name} left for you: {why}")),
        );
    }
    if report.updated.is_empty() && report.skipped.is_empty() {
        body = body.child(
            div()
                .small()
                .text_color(muted)
                .child("Everything installed from the store is current."),
        );
    }
    widgets::card(cx)
        .flex_row()
        .items_start()
        .child(body)
        .child(
            Button::new("workshop-report-close")
                .ghost()
                .icon(Icon::new(IconName::Close))
                .tooltip("Close")
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.workshop.report = None;
                    cx.notify();
                })),
        )
}

/// The store did not answer: what went wrong and a way to ask again.
fn unreachable_store(error: &str, cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_3()
        .child(crate::ui::empty(
            IconName::Inbox,
            "The store could not be read",
            error,
            cx,
        ))
        .child(
            Button::new("workshop-retry")
                .outline()
                .label("Retry")
                .on_click(cx.listener(|ws, _, _, cx| ws.load_store(cx))),
        )
}

impl Workspace {
    /// Reads the store in the chosen order and category, and the skill packs here.
    pub fn load_store(&mut self, cx: &mut Context<Self>) {
        if self.workshop.loading {
            return;
        }
        self.workshop.loading = true;
        let (order, category) = (self.workshop.order, self.workshop.category.clone());
        let all = category.is_none();
        self.core.call(
            cx,
            move |client| async move { client.plugins().store(order, category).await },
            move |ws, result, cx| {
                let shop = &mut ws.workshop;
                shop.loading = false;
                match result {
                    Ok(list) => {
                        if all {
                            shop.categories = categories(&list.listings);
                        }
                        shop.listings = Some(list.listings);
                        shop.picks = list.picks;
                        shop.failed_sources = list.failed;
                        shop.error = None;
                        ws.load_covers(cx);
                    }
                    Err(error) => shop.error = Some(format!("{error:#}")),
                }
                cx.notify();
            },
        );
        self.load_packs(cx);
        cx.notify();
    }

    pub fn load_packs(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.catalog().skill_packs().await },
            |ws, result, cx| match result {
                Ok(packs) => {
                    ws.workshop.packs = Some(packs);
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Switches a pack or a skill of one, then reads the packs and settings again.
    pub fn pack_switch(&mut self, switch: CoreSwitch, on: bool, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            move |client| async move { client.settings().switch(switch, on).await },
            |ws, result, cx| match result {
                Ok(()) => {
                    ws.load_packs(cx);
                    ws.load_settings(cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Updates what asks for nothing more, then shows what was done and left.
    pub fn update_all(&mut self, cx: &mut Context<Self>) {
        if self.workshop.updating {
            return;
        }
        self.workshop.updating = true;
        self.core.call(
            cx,
            |client| async move { client.plugins().update_all().await },
            |ws, result, cx| {
                ws.workshop.updating = false;
                match result {
                    Ok(report) => {
                        ws.workshop.report = Some(report);
                        ws.load_store(cx);
                    }
                    Err(error) => ws.fail(error, cx),
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    /// Opens a listing's window at its cover, with the keys for its gallery.
    pub fn open_listing(&mut self, listing: &Listing, window: &mut Window, cx: &mut Context<Self>) {
        self.workshop.gallery = 0;
        self.workshop.filters = false;
        self.workshop.strip = ScrollHandle::new();
        self.load_screenshots(listing, cx);
        self.modal = Some(Modal::Listing {
            source: listing.source.clone(),
            name: listing.name.clone(),
        });
        self.modal_focus.focus(window, cx);
        cx.notify();
    }

    /// Asks before installing or removing; nothing happens until the yes.
    pub fn confirm_store(&mut self, confirm: Confirm, cx: &mut Context<Self>) {
        self.modal = Some(Modal::Workshop(confirm));
        cx.notify();
    }

    /// The yes of the confirmation window.
    pub fn store_confirmed(&mut self, cx: &mut Context<Self>) {
        let Some(Modal::Workshop(confirm)) = self.modal.take() else {
            return;
        };
        cx.notify();
        match confirm {
            Confirm::Install(listing) => {
                self.store_work(
                    listing.name.clone(),
                    "Installing...",
                    cx,
                    move |client| async move { client.plugins().install(&listing).await },
                );
            }
            Confirm::Remove(name) => {
                let removed = name.clone();
                self.store_work(name, "Removing...", cx, move |client| async move {
                    client.plugins().remove(&removed).await
                });
            }
        }
    }

    /// Runs one install or removal, shows it on the card, then reads the store again.
    fn store_work<F, Fut>(
        &mut self,
        name: String,
        doing: &'static str,
        cx: &mut Context<Self>,
        work: F,
    ) where
        F: FnOnce(std::sync::Arc<dusk_core::client::Client>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        self.workshop.failed.remove(&name);
        self.workshop.busy.insert(name.clone(), doing);
        self.core.call(cx, work, move |ws, result, cx| {
            ws.workshop.busy.remove(&name);
            if let Err(error) = result {
                ws.workshop.failed.insert(name, format!("{error:#}"));
            }
            ws.load_store(cx);
        });
    }
}

/// The categories the listings name, each once, for the chips.
fn categories(listings: &[Listing]) -> Vec<String> {
    let set: std::collections::BTreeSet<&str> = listings
        .iter()
        .map(|listing| listing.category.as_str())
        .filter(|category| !category.is_empty())
        .collect();
    set.into_iter().map(str::to_owned).collect()
}

/// What a listing is, in a word.
pub fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Plugin => "Plugin",
        Kind::Skill => "Skill",
        Kind::Pack => "Pack",
        Kind::Agent => "Agent",
    }
}
