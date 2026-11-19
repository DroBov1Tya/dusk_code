//! The filters: one button beside the search, with a count while any is on, opening a
//! menu of Sort, Category, Tier and Kind; what is on shows as chips under the search.
//! The order and the category are the core's, so changing one reads the store again.

use dusk_protocol::plugin::Order;
use leptos::prelude::*;

use super::{Shop, TIERS, card};
use crate::ui::{Icon, IconName};

const ORDERS: [(Order, &str); 3] = [
    (Order::Trust, "Official first"),
    (Order::Newest, "Newest"),
    (Order::Updated, "Recently updated"),
];

fn active(shop: Shop) -> usize {
    usize::from(shop.category.with(Option::is_some))
        + shop.tiers.with(Vec::len)
        + shop.kinds.with(Vec::len)
        + usize::from(shop.order.get() != Order::Trust)
}

fn flip<T: PartialEq>(set: &mut Vec<T>, item: T) {
    match set.iter().position(|each| *each == item) {
        Some(index) => {
            set.remove(index);
        }
        None => set.push(item),
    }
}

fn set_order(shop: Shop, order: Order) {
    if shop.order.get_untracked() != order {
        shop.order.set(order);
        shop.load();
    }
}

fn set_category(shop: Shop, category: Option<String>) {
    if shop.category.get_untracked() != category {
        shop.category.set(category);
        shop.shelf.set(None);
        shop.load();
    }
}

fn clear(shop: Shop) {
    shop.tiers.set(Vec::new());
    shop.kinds.set(Vec::new());
    let reload =
        shop.category.get_untracked().is_some() || shop.order.get_untracked() != Order::Trust;
    shop.category.set(None);
    shop.order.set(Order::Trust);
    if reload {
        shop.load();
    }
}

pub fn button(shop: Shop) -> impl IntoView {
    view! {
        <div class="filter-anchor">
            <button class="icon" class:active=move || { shop.menu.get() || active(shop) > 0 }
                title="Filters and order" aria-label="Filters and order"
                aria-expanded=move || shop.menu.get().to_string()
                on:click=move |_| shop.menu.update(|open| *open = !*open)>
                <Icon name=IconName::Filter />
            </button>
            {move || (active(shop) > 0).then(|| view! { <span class="count">{active(shop)}</span> })}
            {move || shop.menu.get().then(|| menu(shop))}
        </div>
    }
}

fn menu(shop: Shop) -> impl IntoView {
    let sorts = ORDERS
        .map(|(order, label)| {
            view! {
                <label class="menu-row"><input type="radio" name="workshop-sort"
                    prop:checked=move || shop.order.get() == order
                    on:change=move |_| set_order(shop, order) />{label}</label>
            }
        })
        .into_iter()
        .collect_view();
    let categories = shop
        .categories
        .get()
        .into_iter()
        .map(|category| {
            let (shown, picked) = (category.clone(), category.clone());
            view! {
                <label class="menu-row"><input type="checkbox"
                    prop:checked=move || shop.category.get().as_deref() == Some(shown.as_str())
                    on:change=move |ev| set_category(shop, event_target_checked(&ev).then(|| picked.clone())) />
                    {capitalize(&category)}</label>
            }
        })
        .collect_view();
    let categories = (!shop.categories.with(Vec::is_empty))
        .then(|| view! { <p class="menu-title">"Category"</p>{categories} });
    let tiers = TIERS
        .map(|tier| {
            view! {
                <label class="menu-row"><input type="checkbox"
                    prop:checked=move || shop.tiers.with(|tiers| tiers.contains(&tier))
                    on:change=move |_| { shop.tiers.update(|tiers| flip(tiers, tier)); shop.shelf.set(None); } />
                    {card::mark(tier)}</label>
            }
        })
        .into_iter()
        .collect_view();
    // A shelf of one kind has none to choose.
    let kinds = Some(shop.scope.kinds())
        .filter(|kinds| kinds.len() > 1)
        .unwrap_or_default();
    let kinds = (!kinds.is_empty()).then(|| {
        let rows = kinds
            .into_iter()
            .map(|kind| {
                view! {
                    <label class="menu-row"><input type="checkbox"
                        prop:checked=move || shop.kinds.with(|kinds| kinds.contains(&kind))
                        on:change=move |_| { shop.kinds.update(|kinds| flip(kinds, kind)); shop.shelf.set(None); } />
                        {format!("{}s", card::kind_word(kind))}</label>
                }
            })
            .collect_view();
        view! { <p class="menu-title">"Kind"</p>{rows} }
    });
    view! {
        <div class="filter-menu" role="dialog" aria-label="Filters"
            on:keydown=move |ev| if ev.key() == "Escape" { shop.menu.set(false) }>
            <div class="menu-list">
                <p class="menu-title">"Sort"</p>{sorts}
                {categories}
                <p class="menu-title">"Tier"</p>{tiers}
                {kinds}
            </div>
            <div class="menu-foot">
                <button class="ghost quiet" on:click=move |_| clear(shop)>"Clear all"</button>
                <button class="ghost" on:click=move |_| shop.menu.set(false)>"Done"</button>
            </div>
        </div>
    }
}

/// The filters that are on, each a chip with a cross; nothing while none is.
pub fn chips(shop: Shop) -> Option<AnyView> {
    if active(shop) == 0 {
        return None;
    }
    let chip = |label: String, remove: Box<dyn Fn() + 'static>| {
        view! {
            <button class="chip active" title="Remove this filter" on:click=move |_| remove()>
                {label}<Icon name=IconName::Close /></button>
        }
    };
    let mut chips = Vec::new();
    let order = shop.order.get();
    if order != Order::Trust {
        let label = ORDERS
            .iter()
            .find(|(each, _)| *each == order)
            .map_or("", |(_, word)| word);
        chips.push(chip(
            label.to_owned(),
            Box::new(move || set_order(shop, Order::Trust)),
        ));
    }
    if let Some(category) = shop.category.get() {
        chips.push(chip(
            capitalize(&category),
            Box::new(move || set_category(shop, None)),
        ));
    }
    for tier in shop.tiers.get() {
        chips.push(chip(
            card::word(tier).to_owned(),
            Box::new(move || shop.tiers.update(|tiers| flip(tiers, tier))),
        ));
    }
    for kind in shop.kinds.get() {
        chips.push(chip(
            format!("{}s", card::kind_word(kind)),
            Box::new(move || shop.kinds.update(|kinds| flip(kinds, kind))),
        ));
    }
    Some(
        view! {
            <div class="store-chips">{chips.collect_view()}
                <button class="ghost quiet" on:click=move |_| clear(shop)>"Clear all"</button></div>
        }
        .into_any(),
    )
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}
