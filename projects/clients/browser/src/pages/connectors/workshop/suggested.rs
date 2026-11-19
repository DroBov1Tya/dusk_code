//! A card of Workshop listings the agent put in a chat, as the desktop app draws it: the
//! heading, then a row per listing with its glyph, name, kind, tier and a line of what it
//! is, and the Workshop card's own slot at the right. A browser installs nothing (the
//! server refuses it), so where the app has Add the slot says where installing happens,
//! as on the Workshop page. Whether a row is installed is read from the store now; the
//! card only says what was suggested. A click on a row opens the Workshop's page of its
//! kind. All listing text is drawn as text nodes.

use dusk_protocol::plugin::{Kind, Listing, Order, Suggested, Suggestion};
use leptos::prelude::*;

use super::{
    card::{art, kind_word, mark, slot},
    placeholder::Picture,
};
use crate::{
    api,
    pages::connectors::Tab,
    state::{View, state},
};

#[component]
pub fn SuggestedCard(card: Suggestion) -> impl IntoView {
    // A shared chat may not read the store: its rows then say so instead of waiting.
    let store = RwSignal::new(None::<Result<Vec<Listing>, String>>);
    leptos::task::spawn_local(async move {
        let read = api::store(Order::Trust, None).await;
        store.set(Some(read.map(|list| list.listings)));
    });
    let rows = card
        .listings
        .into_iter()
        .map(|row| view! { <SuggestedRow row store /> })
        .collect_view();
    let (label, heading) = (card.label.clone(), card.label);
    view! {
        <section class="suggested" aria-label=label>
            <h3>{heading}</h3>
            {rows}
        </section>
    }
}

#[component]
fn SuggestedRow(
    row: Suggested,
    store: RwSignal<Option<Result<Vec<Listing>, String>>>,
) -> impl IntoView {
    let about = match row.skills {
        0 => row.description.clone(),
        count => format!("{count} skills \u{b7} {}", row.description),
    };
    let tab = page(row.kind);
    let found = row.clone();
    let note = |text: &'static str| view! { <span class="slot-note">{text}</span> }.into_any();
    let shown = move || {
        store.with(|read| match read {
            None => note("Reading..."),
            Some(Err(_)) => note("Workshop not readable here"),
            Some(Ok(listings)) => match found.find(listings) {
                Some(listing) => slot(listing),
                None => note("Not in the Workshop now"),
            },
        })
    };
    view! {
        <div class="suggested-row">
            <div class="tile">{art(row.kind, &row.name, Picture::Missing)}</div>
            <div class="suggested-text">
                <div class="suggested-name">
                    <button class="open" title=row.name.clone()
                        on:click=move |_| state().show(View::Connectors(tab))>{row.name.clone()}</button>
                    <span class="kind">{kind_word(row.kind)}</span>
                    {mark(row.tier)}
                </div>
                <p class="desc">{about}</p>
            </div>
            {shown}
        </div>
    }
}

/// The Workshop page that lists a kind: single skills on Skills, the rest on Plugins.
fn page(kind: Kind) -> Tab {
    match kind {
        Kind::Skill => Tab::Skills,
        Kind::Plugin | Kind::Pack | Kind::Agent => Tab::Plugins,
    }
}
