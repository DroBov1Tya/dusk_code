//! The hero: the official picks one at a time, as a card 280 px high across the grid,
//! the cover at 40:21 on the left (on top when the page is narrow) and the panel with
//! the name, three lines of text, the facts, the state, Details, and previous, dots and
//! next. It never moves by itself; a change fades in. The controls stay in place while
//! the pick changes, so the keyboard focus is kept.

use dusk_protocol::plugin::Listing;
use leptos::prelude::*;

use super::{Shop, card};
use crate::ui::{Icon, IconName};

pub fn hero(shop: Shop, picks: Vec<Listing>) -> impl IntoView {
    let count = picks.len();
    let picks = StoredValue::new(picks);
    let at = move || shop.hero.get() % count;
    let current = move || picks.with_value(|picks| picks[at()].clone());
    let step = move |by: usize| {
        shop.hero
            .update(|index| *index = (*index % count + by) % count)
    };
    let open = move |_| {
        let index = shop.hero.get_untracked() % count;
        picks.with_value(|picks| shop.open_listing(&picks[index]));
    };
    let dots = (0..count)
        .map(|each| view! { <i class:on=move || at() == each></i> })
        .collect_view();
    view! {
        <section class="hero hero-card" aria-label="Featured">
            <div class="hero-art">{move || card::cover(shop, &current())}</div>
            <div class="hero-panel">
                {move || text(&current(), at(), count)}
                <div class="hero-foot">
                    {move || card::slot(&current())}
                    <button class="ghost" on:click=open>"Details"</button>
                    {(count > 1).then(|| view! {
                        <span class="pager">
                            <button class="icon" aria-label="Previous pick" on:click=move |_| step(count - 1)>
                                <Icon name=IconName::ChevronLeft /></button>
                            <span class="dots" aria-hidden="true">{dots}</span>
                            <button class="icon" aria-label="Next pick" on:click=move |_| step(1)>
                                <Icon name=IconName::ChevronRight /></button>
                        </span>
                    })}
                </div>
            </div>
        </section>
    }
}

/// Where it comes from and which of the picks, the name, the text and the facts.
fn text(listing: &Listing, index: usize, count: usize) -> AnyView {
    let source = match listing.source.as_str() {
        "" => "duskcode",
        source => source,
    };
    view! {
        <div class="hero-text">
            <span class="vendor">{format!("From {source} \u{b7} {} of {count}", index + 1)}</span>
            <h2 title=listing.name.clone()>{listing.name.clone()}</h2>
            <p>{listing.description.clone()}</p>
            {card::facts(listing)}
        </div>
    }
    .into_any()
}

/// The category and tags as quiet chips.
pub fn tags(listing: &Listing) -> Option<AnyView> {
    let words: Vec<String> = std::iter::once(&listing.category)
        .chain(&listing.tags)
        .filter(|word| !word.is_empty())
        .cloned()
        .collect();
    (!words.is_empty()).then(|| {
        view! { <div class="tags">{words.into_iter().map(|word| view! { <span class="tag">{word}</span> }).collect_view()}</div> }
            .into_any()
    })
}
