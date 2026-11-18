//! Featured: the official picks as a wide hero, one at a time: the cover on the left
//! fading into the panel with the name, what it is, its tags and facts; a strip of the
//! picks' covers under it, and arrows. It does not move by itself.

use dusk_protocol::plugin::Listing;
use leptos::prelude::*;

use super::{Shop, card};
use crate::ui::{Icon, IconName};

pub fn hero(shop: Shop, picks: Vec<Listing>) -> impl IntoView {
    let count = picks.len();
    let index = shop.hero.get() % count;
    let listing = picks[index].clone();
    let source = match listing.source.as_str() {
        "" => "duskcode".to_owned(),
        source => source.to_owned(),
    };
    let thumbs = picks
        .iter()
        .enumerate()
        .map(|(each, pick)| {
            view! {
                <button class="thumb" class:active=each == index aria-label=format!("Show {}", pick.name)
                    on:click=move |_| shop.hero.set(each)>
                    {card::art(&pick.name, pick.tier, shop.cover(pick))}</button>
            }
        })
        .collect_view();
    let opened = listing.clone();
    let step = move |by: usize| shop.hero.set((index + by) % count);
    view! {
        <section class="hero" aria-label="Featured">
            <div class="hero-card">
                <div class="hero-art">
                    {card::art(&listing.name, listing.tier, shop.cover(&listing))}
                    {(count > 1).then(|| view! {
                        <button class="round back" aria-label="Previous" on:click=move |_| step(count - 1)><Icon name=IconName::ChevronLeft /></button>
                        <button class="round next" aria-label="Next" on:click=move |_| step(1)><Icon name=IconName::ChevronRight /></button>
                    })}
                </div>
                <div class="hero-panel">
                    <span class="vendor">{format!("Featured \u{b7} from {source}")}</span>
                    <h2>{listing.name.clone()}</h2>
                    <p class="muted">{listing.description.clone()}</p>
                    {card::facts(&listing)}
                    {tags(&listing)}
                    <div class="hero-foot">
                        {card::state(&listing)}
                        <span class="grow"></span>
                        <button class="ghost" on:click=move |_| shop.open_listing(&opened)>"Details"</button>
                    </div>
                </div>
            </div>
            {(count > 1).then(|| view! { <div class="hero-strip">{thumbs}</div> })}
        </section>
    }
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
