//! The filters: one button beside the search, with a count while any is on, opening a
//! menu of Category, Tier and Kind to check and the order to pick; what is on shows as
//! chips under the search, each removed with its cross. A category and an order are the
//! core's to apply, so changing one reads the store again; tiers and kinds only narrow
//! what is shown here, as the core has no such filter.

use std::time::{Duration, Instant};

use dusk_protocol::plugin::Order;
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::tier;
use crate::{
    ui::{Type, controls::choice, icons, size, widgets},
    workspace::Workspace,
};

const ORDERS: [(Order, &str); 3] = [
    (Order::Trust, "Official first"),
    (Order::Newest, "Newest"),
    (Order::Updated, "Recently updated"),
];

/// Two columns: the order, tier and kind beside the categories, so all show at once.
const MENU_WIDTH: f32 = 480.;
const MENU_HEIGHT: f32 = 520.;
/// A click on the button this soon after a click outside closed the menu is that click.
const REOPEN: Duration = Duration::from_millis(250);

pub fn order_word(order: Order) -> &'static str {
    ORDERS
        .iter()
        .find(|(each, _)| *each == order)
        .map_or("", |(_, word)| word)
}

impl Workspace {
    fn toggle_filters(&mut self, cx: &mut Context<Self>) {
        let shop = &mut self.workshop;
        let just_closed = shop
            .filters_closed
            .is_some_and(|closed| closed.elapsed() < REOPEN);
        shop.filters = !shop.filters && !just_closed;
        cx.notify();
    }

    fn set_category(&mut self, category: Option<String>, cx: &mut Context<Self>) {
        if self.workshop.category != category {
            self.workshop.category = category;
            self.workshop.shelf = None;
            self.load_store(cx);
        }
    }

    fn set_order(&mut self, order: Order, cx: &mut Context<Self>) {
        if self.workshop.order != order {
            self.workshop.order = order;
            self.load_store(cx);
        }
    }
}

/// Puts `item` in or out of `set`.
fn flip<T: PartialEq>(set: &mut Vec<T>, item: T) {
    match set.iter().position(|each| *each == item) {
        Some(index) => {
            set.remove(index);
        }
        None => set.push(item),
    }
}

/// How many filters are on, the order counted when it is not the default.
fn active(ws: &Workspace) -> usize {
    let shop = &ws.workshop;
    usize::from(shop.category.is_some())
        + shop.tiers.len()
        + shop.kinds.len()
        + usize::from(shop.order != Order::Trust)
}

/// The Filters button with its count, and its menu under it while open.
pub fn button(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let on = active(ws);
    let theme = cx.theme();
    let (accent, on_accent) = (theme.primary, theme.primary_foreground);
    let button = choice(
        Button::new("workshop-filters"),
        ws.workshop.filters || on > 0,
        cx,
    )
    .icon(Icon::default().path(icons::FILTER))
    .tooltip("Filters and order")
    .on_click(cx.listener(|ws, _, _, cx| ws.toggle_filters(cx)));
    div()
        .relative()
        .flex_none()
        .child(button)
        .when(on > 0, |wrap| {
            wrap.child(
                div()
                    .absolute()
                    .top(px(-size::GAP_XS))
                    .right(px(-size::GAP_XS))
                    .min_w(px(size::ICON))
                    .h(px(size::ICON))
                    .px(px(size::GAP_XS))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(accent)
                    .text_color(on_accent)
                    .meta()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(on.to_string()),
            )
        })
        .when(ws.workshop.filters, |wrap| wrap.child(menu(ws, cx)))
}

fn menu(ws: &Workspace, cx: &mut Context<Workspace>) -> Deferred {
    let shop = &ws.workshop;
    let theme = cx.theme();
    let (border, surface) = (theme.border, theme.popover);
    let column = |id: &'static str| {
        div()
            .id(id)
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .p(px(size::GAP_XS))
            .max_h(px(MENU_HEIGHT))
            .overflow_y_scroll()
    };
    let mut list = column("workshop-filters-main");
    let mut categories = column("workshop-filters-categories");
    list = list.child(section("Sort", cx));
    for (order, label) in ORDERS {
        list = list.child(
            row(
                format!("sort-{label}"),
                mark(Mark::Radio, shop.order == order, cx),
                cx,
            )
            .child(label)
            .on_click(cx.listener(move |ws, _, _, cx| ws.set_order(order, cx))),
        );
    }
    list = list.child(section("Tier", cx));
    for each in tier::ALL {
        list = list.child(
            row(
                format!("tier-{}", tier::word(each)),
                mark(Mark::Check, shop.tiers.contains(&each), cx),
                cx,
            )
            .child(tier::mark(each, cx))
            .on_click(cx.listener(move |ws, _, _, cx| {
                flip(&mut ws.workshop.tiers, each);
                ws.workshop.shelf = None;
                cx.notify();
            })),
        );
    }
    // The Skills page lists one kind, so there it has none to choose.
    let kinds = match shop.scope {
        super::Scope::Plugins => shop.scope.kinds(),
        super::Scope::Skills => Vec::new(),
    };
    if !kinds.is_empty() {
        list = list.child(section("Kind", cx));
    }
    for each in kinds {
        list = list.child(
            row(
                format!("kind-{}", super::kind_word(each)),
                mark(Mark::Check, shop.kinds.contains(&each), cx),
                cx,
            )
            .child(format!("{}s", super::kind_word(each)))
            .on_click(cx.listener(move |ws, _, _, cx| {
                flip(&mut ws.workshop.kinds, each);
                ws.workshop.shelf = None;
                cx.notify();
            })),
        );
    }
    if !shop.categories.is_empty() {
        categories = categories.child(section("Category", cx));
        for category in &shop.categories {
            let chosen = shop.category.as_deref() == Some(category);
            let picked = category.clone();
            categories = categories.child(
                row(format!("cat-{category}"), mark(Mark::Check, chosen, cx), cx)
                    .child(widgets::capitalize(category))
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        let next = (!chosen).then(|| picked.clone());
                        ws.set_category(next, cx)
                    })),
            );
        }
    }
    let panel = div()
        .id("workshop-filters-panel")
        .absolute()
        .top(px(size::CONTROL + size::GAP_XS))
        .right_0()
        .w(px(if shop.categories.is_empty() {
            MENU_WIDTH / 2.
        } else {
            MENU_WIDTH
        }))
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(surface)
        .shadow_lg()
        .occlude()
        .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
            ws.workshop.filters = false;
            ws.workshop.filters_closed = Some(Instant::now());
            cx.notify();
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .child(list)
                .when(!shop.categories.is_empty(), |row| row.child(categories)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .p(px(size::GAP_S))
                .border_t_1()
                .border_color(border)
                .child(
                    Button::new("workshop-filters-clear")
                        .ghost()
                        .label("Clear all")
                        .on_click(cx.listener(|ws, _, _, cx| clear(ws, cx))),
                ),
        );
    widgets::on_top(widgets::appear("workshop-filters-open", panel))
}

fn clear(ws: &mut Workspace, cx: &mut Context<Workspace>) {
    ws.workshop.tiers.clear();
    ws.workshop.kinds.clear();
    let reload = ws.workshop.category.take().is_some() || ws.workshop.order != Order::Trust;
    ws.workshop.order = Order::Trust;
    if reload {
        ws.load_store(cx);
    }
    cx.notify();
}

fn section(title: &'static str, cx: &App) -> Div {
    div()
        .flex_none()
        .px(px(size::GAP_M))
        .pt(px(size::GAP_M))
        .pb(px(size::GAP_XS))
        .group_name()
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

/// A menu row: its mark in a fixed column, then what it names.
fn row(id: String, mark: Div, cx: &App) -> Stateful<Div> {
    // A row keeps its height in the scrolled list instead of shrinking to fit it.
    widgets::menu_item(SharedString::from(format!("workshop-filter-{id}")), cx)
        .flex_none()
        .gap(px(size::GAP_S))
        .small()
        .child(mark)
}

enum Mark {
    Check,
    Radio,
}

/// A box with a check, or a ring with a dot, an icon's size.
fn mark(kind: Mark, on: bool, cx: &App) -> Div {
    let theme = cx.theme();
    let (accent, on_accent, border) = (theme.primary, theme.primary_foreground, theme.input);
    let frame = div()
        .size(px(size::ICON))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .border_1();
    match (kind, on) {
        (Mark::Check, true) => frame.rounded_sm().border_color(accent).bg(accent).child(
            Icon::new(IconName::Check)
                .size(px(size::ICON - size::GAP_XS))
                .text_color(on_accent),
        ),
        (Mark::Check, false) => frame.rounded_sm().border_color(border),
        (Mark::Radio, true) => frame
            .rounded_full()
            .border_color(accent)
            .child(div().size(px(size::GAP_S)).rounded_full().bg(accent)),
        (Mark::Radio, false) => frame.rounded_full().border_color(border),
    }
}

/// The filters that are on, as chips under the search; none while none is.
pub fn chips(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    if active(ws) == 0 {
        return None;
    }
    let shop = &ws.workshop;
    let mut row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(size::GAP_S));
    if shop.order != Order::Trust {
        row = row.child(
            chip("order", order_word(shop.order).to_owned(), cx)
                .on_click(cx.listener(|ws, _, _, cx| ws.set_order(Order::Trust, cx))),
        );
    }
    if let Some(category) = &shop.category {
        row = row.child(
            chip("category", widgets::capitalize(category), cx)
                .on_click(cx.listener(|ws, _, _, cx| ws.set_category(None, cx))),
        );
    }
    for each in shop.tiers.clone() {
        row = row.child(
            chip(tier::word(each), tier::word(each).to_owned(), cx).on_click(cx.listener(
                move |ws, _, _, cx| {
                    flip(&mut ws.workshop.tiers, each);
                    cx.notify();
                },
            )),
        );
    }
    for each in shop.kinds.clone() {
        let word = super::kind_word(each);
        row = row.child(chip(word, format!("{word}s"), cx).on_click(cx.listener(
            move |ws, _, _, cx| {
                flip(&mut ws.workshop.kinds, each);
                cx.notify();
            },
        )));
    }
    Some(
        row.child(
            Button::new("workshop-chips-clear")
                .ghost()
                .label("Clear all")
                .on_click(cx.listener(|ws, _, _, cx| clear(ws, cx))),
        ),
    )
}

/// A filter that is on: its word and a cross that takes it off.
fn chip(id: &str, label: String, cx: &App) -> Button {
    choice(
        Button::new(SharedString::from(format!("workshop-chip-{id}"))),
        true,
        cx,
    )
    .label(label)
    .child(Icon::new(IconName::Close).size(px(size::ICON)))
    .tooltip("Remove this filter")
}
