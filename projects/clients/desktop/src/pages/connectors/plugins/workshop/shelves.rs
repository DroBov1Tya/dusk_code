//! The shelves under the hero: one per tier, then the core's picks of the newest, the
//! latest updated and the most starred, then the packs here that no source lists. Each
//! shows one row of capsules, with arrows when it holds more and "See all" for the whole
//! shelf as a grid. A search, a filter or another order shows the results as one grid in
//! the core's order instead.

use dusk_protocol::{
    catalog::SkillPack,
    plugin::{Kind, Listing, Tier},
};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, IconName,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{Grid, capsule, filters, grid, tier};
use crate::{
    pages::connectors::matches,
    ui::{Type, size},
    workspace::Workspace,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shelf {
    Tier(Tier),
    Newest,
    Updated,
    Stars,
    /// Local listings and the packs here that no source lists.
    Local,
}

impl Shelf {
    fn title(self) -> &'static str {
        match self {
            Self::Tier(each) => tier::word(each),
            Self::Newest => "New",
            Self::Updated => "Recently updated",
            Self::Stars => "Top rated",
            Self::Local => "Built-in and local",
        }
    }

    fn about(self) -> &'static str {
        match self {
            Self::Tier(each) => tier::about(each),
            Self::Newest => "The latest additions to every source.",
            Self::Updated => "What changed most recently.",
            Self::Stars => "What people starred most.",
            Self::Local => "Found on this machine; the built-in pack comes with duskcode.",
        }
    }

    fn icon(self, cx: &App) -> Icon {
        let muted = cx.theme().muted_foreground;
        match self {
            Self::Tier(each) => tier::icon(each, size::ICON_LARGE, cx),
            Self::Local => tier::icon(Tier::Local, size::ICON_LARGE, cx),
            Self::Newest => Icon::new(IconName::Calendar)
                .size(px(size::ICON_LARGE))
                .text_color(muted),
            Self::Updated => Icon::default()
                .path(crate::ui::icons::REFRESH)
                .size(px(size::ICON_LARGE))
                .text_color(muted),
            Self::Stars => Icon::new(IconName::Star)
                .size(px(size::ICON_LARGE))
                .text_color(muted),
        }
    }
}

/// What a shelf holds.
#[derive(Clone, Copy)]
enum Item<'a> {
    Listing(&'a Listing),
    Pack(&'a SkillPack),
}

const ORDER: [Shelf; 7] = [
    Shelf::Tier(Tier::Official),
    Shelf::Tier(Tier::Verified),
    Shelf::Tier(Tier::Community),
    Shelf::Newest,
    Shelf::Updated,
    Shelf::Stars,
    Shelf::Local,
];

pub fn render(
    ws: &Workspace,
    shown: &[&Listing],
    query: &str,
    grid: Grid,
    cx: &mut Context<Workspace>,
) -> Div {
    let shop = &ws.workshop;
    let page = div().flex().flex_col().gap(px(size::GAP_XXL));
    if !super::browsing(shop, query) {
        let mut items: Vec<Item> = shown.iter().copied().map(Item::Listing).collect();
        items.extend(local_packs(ws, query).into_iter().map(Item::Pack));
        if items.is_empty() {
            return page.child(nothing(ws, cx));
        }
        let title = match shop.order {
            dusk_protocol::plugin::Order::Trust => "Results".to_owned(),
            order => format!("Results \u{b7} {}", filters::order_word(order)),
        };
        return page.child(
            div()
                .flex()
                .flex_col()
                .gap(px(grid::GAP))
                .child(heading(None, &title, items.len(), None, cx))
                .child(wrap(ws, &items, grid, cx)),
        );
    }
    if let Some(shelf) = shop.shelf {
        let items = items(ws, shelf, shown, query);
        let back = Button::new("workshop-shelves")
            .ghost()
            .icon(Icon::new(IconName::ChevronLeft))
            .label("All shelves")
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.workshop.shelf = None;
                cx.notify();
            }));
        return page.child(
            div()
                .flex()
                .flex_col()
                .gap(px(grid::GAP))
                .child(div().flex().child(back))
                .child(heading(Some(shelf), shelf.title(), items.len(), None, cx))
                .child(wrap(ws, &items, grid, cx)),
        );
    }
    let mut page = page;
    let mut any = false;
    for shelf in ORDER {
        let items = items(ws, shelf, shown, query);
        if items.is_empty() {
            continue;
        }
        any = true;
        page = page.child(row(ws, shelf, &items, grid, cx));
    }
    if !any {
        return page.child(nothing(ws, cx));
    }
    page
}

/// One shelf: its heading with arrows and "See all", and the capsules that fit a row.
fn row(
    ws: &Workspace,
    shelf: Shelf,
    items: &[Item],
    grid: Grid,
    cx: &mut Context<Workspace>,
) -> Div {
    let count = items.len();
    let more = count > grid.columns;
    let last = count.saturating_sub(grid.columns);
    let first = ws
        .workshop
        .scrolled
        .get(&shelf)
        .copied()
        .unwrap_or(0)
        .min(last);
    let controls = more.then(|| {
        let step = grid.columns;
        let key = format!("{shelf:?}");
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(size::GAP_XS))
            .child(
                Button::new(SharedString::from(format!("shelf-back-{key}")))
                    .ghost()
                    .icon(Icon::new(IconName::ChevronLeft))
                    .tooltip("Back")
                    .disabled(first == 0)
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.workshop
                            .scrolled
                            .insert(shelf, first.saturating_sub(step));
                        cx.notify();
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("shelf-next-{key}")))
                    .ghost()
                    .icon(Icon::new(IconName::ChevronRight))
                    .tooltip("More")
                    .disabled(first >= last)
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.workshop.scrolled.insert(shelf, (first + step).min(last));
                        cx.notify();
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("shelf-all-{key}")))
                    .ghost()
                    .label("See all")
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.workshop.shelf = Some(shelf);
                        ws.shelf_scroll.set_offset(Point::default());
                        cx.notify();
                    })),
            )
    });
    let mut capsules = div().flex().flex_row().gap(px(grid::GAP));
    for item in items.iter().skip(first).take(grid.columns) {
        capsules = capsules.child(item_capsule(ws, *item, grid.capsule, cx));
    }
    div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_M))
        .child(heading(Some(shelf), shelf.title(), count, controls, cx))
        .child(capsules)
}

/// The capsules wrapped on the page's columns.
fn wrap(ws: &Workspace, items: &[Item], grid: Grid, cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(grid::GAP))
        .children(
            items
                .iter()
                .map(|item| item_capsule(ws, *item, grid.capsule, cx)),
        )
}

fn item_capsule(
    ws: &Workspace,
    item: Item,
    width: f32,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    match item {
        Item::Listing(listing) => capsule::render(ws, listing, width, cx),
        Item::Pack(pack) => capsule::pack(pack, width, cx),
    }
}

/// A shelf's icon, name and count with one line on what it holds, and its controls at
/// the right end, all on a control's height.
fn heading(
    shelf: Option<Shelf>,
    title: &str,
    count: usize,
    controls: Option<Div>,
    cx: &App,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut name = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_S))
        .h(px(size::CONTROL));
    if let Some(shelf) = shelf {
        name = name.child(shelf.icon(cx));
    }
    let name = name
        .child(
            div()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_owned()),
        )
        .child(div().meta().text_color(muted).child(count.to_string()));
    // The controls stand on the name's line, not centred on the line under it.
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(size::GAP_L))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(name)
                .when_some(shelf, |column, shelf| {
                    column.child(div().meta().text_color(muted).child(shelf.about()))
                }),
        )
        .child(div().flex_1())
        .children(controls)
}

/// What a shelf holds while browsing.
fn items<'a>(ws: &'a Workspace, shelf: Shelf, shown: &[&'a Listing], query: &str) -> Vec<Item<'a>> {
    let shop = &ws.workshop;
    let all = shop.listings.as_deref().unwrap_or_default();
    let picked = |indices: &[usize]| -> Vec<Item<'a>> {
        indices
            .iter()
            .filter_map(|&index| all.get(index))
            .map(Item::Listing)
            .collect()
    };
    match shelf {
        Shelf::Tier(each) => shown
            .iter()
            .copied()
            .filter(|listing| listing.tier == each)
            .map(Item::Listing)
            .collect(),
        Shelf::Newest => picked(&shop.picks.newest),
        Shelf::Updated => picked(&shop.picks.updated),
        Shelf::Stars => picked(&shop.picks.stars),
        Shelf::Local => shown
            .iter()
            .copied()
            .filter(|listing| listing.tier == Tier::Local)
            .map(Item::Listing)
            .chain(local_packs(ws, query).into_iter().map(Item::Pack))
            .collect(),
    }
}

/// Packs here that no listing of the store names, unless a filter leaves them out.
fn local_packs<'a>(ws: &'a Workspace, query: &str) -> Vec<&'a SkillPack> {
    let shop = &ws.workshop;
    let listed = |name: &str| {
        shop.listings
            .iter()
            .flatten()
            .any(|listing| listing.kind == Kind::Pack && listing.name == name)
    };
    let shown = (shop.tiers.is_empty() || shop.tiers.contains(&Tier::Local))
        && (shop.kinds.is_empty() || shop.kinds.contains(&Kind::Pack))
        && shop.category.is_none();
    if !shown {
        return Vec::new();
    }
    shop.packs
        .iter()
        .flatten()
        .filter(|pack| !listed(&pack.name))
        .filter(|pack| matches(query, &[&pack.name, &pack.title, &pack.description]))
        .collect()
}

fn nothing(ws: &Workspace, cx: &App) -> Div {
    let shop = &ws.workshop;
    let (title, hint) = match shop.listings.as_ref().is_some_and(|all| all.is_empty()) {
        true if !shop.failed_sources.is_empty() => {
            ("No source could be read", "Try again when online.")
        }
        true => ("The store is empty", "Nothing is offered yet."),
        false => ("Nothing matches", "Try another word or filter."),
    };
    crate::ui::empty(IconName::Inbox, title, hint, cx)
}
