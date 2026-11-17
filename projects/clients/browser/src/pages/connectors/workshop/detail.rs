//! A listing's window, as a store page: the gallery (the cover, then the screenshots with
//! their captions, a strip to jump, arrows and the Left and Right keys) and the whole
//! description on the left; its name, source, facts, tags, state, warnings, what it may
//! do, its page and its dates on the right. Nothing is installed from here.

use dusk_protocol::plugin::{Kind, Listing};
use leptos::prelude::*;

use super::{Shop, card, hero, shots_key};
use crate::ui::{Icon, IconName};

pub fn window(shop: Shop, source: String, name: String) -> impl IntoView {
    let close = move || shop.window.set(None);
    let Some(listing) = shop.find(&source, &name) else {
        return view! { <div class="overlay" on:click=move |_| close()></div> }.into_any();
    };
    let count = 1 + listing.screenshots.len();
    let step = move |by: isize| {
        shop.gallery
            .update(|index| *index = (*index as isize + by).clamp(0, count as isize - 1) as usize)
    };
    // The window takes the keys as it opens, so Left and Right step the gallery at once.
    let node = NodeRef::<leptos::html::Div>::new();
    Effect::new(move || {
        if let Some(window) = node.get() {
            let _ = window.focus();
        }
    });
    view! {
        <div class="overlay" on:click=move |_| close()>
            <div class="window store-window" node_ref=node role="dialog" aria-modal="true" aria-label=listing.name.clone()
                tabindex="-1" on:click=|ev| ev.stop_propagation()
                on:keydown=move |ev| match ev.key().as_str() {
                    "ArrowLeft" => step(-1),
                    "ArrowRight" => step(1),
                    "Escape" => close(),
                    _ => {}
                }>
                <div class="window-head"><b>{listing.name.clone()}</b><span class="grow"></span>
                    <button class="icon" aria-label="Close" on:click=move |_| close()><Icon name=IconName::Close /></button></div>
                <div class="store-page">
                    <div class="store-main">
                        {gallery(shop, &listing, step)}
                        <h3>"About"</h3>
                        <p>{listing.description.clone()}</p>
                    </div>
                    {side(shop, &listing)}
                </div>
            </div>
        </div>
    }
    .into_any()
}

fn gallery(shop: Shop, listing: &Listing, step: impl Fn(isize) + Copy + 'static) -> impl IntoView {
    let shots = shop
        .shots
        .with(|shots| shots.get(&shots_key(listing)).cloned().unwrap_or_default());
    let mut media: Vec<(Option<String>, Option<String>)> = vec![(shop.cover(listing), None)];
    for (url, about) in shots.into_iter().zip(&listing.screenshots) {
        media.push(((!url.is_empty()).then_some(url), about.caption.clone()));
    }
    let count = media.len();
    let index = shop.gallery.get().min(count - 1);
    let (shown, caption) = media[index].clone();
    let picture = |url: Option<String>, name: &str, listing: &Listing| match url {
        Some(url) => view! { <div class="art fit"><img src=url alt="" /></div> }.into_any(),
        None => card::art(name, listing.tier, None),
    };
    let thumbs = media
        .iter()
        .enumerate()
        .map(|(each, (url, _))| {
            view! {
                <button class="thumb" class:active=each == index aria-label=format!("Picture {}", each + 1)
                    on:click=move |_| shop.gallery.set(each)>{picture(url.clone(), &listing.name, listing)}</button>
            }
        })
        .collect_view();
    view! {
        <div class="media">
            <div class="stage">
                {picture(shown, &listing.name, listing)}
                {(count > 1).then(|| view! {
                    <button class="round back" aria-label="Previous picture" on:click=move |_| step(-1)><Icon name=IconName::ChevronLeft /></button>
                    <button class="round next" aria-label="Next picture" on:click=move |_| step(1)><Icon name=IconName::ChevronRight /></button>
                })}
            </div>
            {caption.map(|text| view! { <p class="caption muted">{text}</p> })}
            {(count > 1).then(|| view! { <div class="strip">{thumbs}</div> })}
        </div>
    }
}

fn side(shop: Shop, listing: &Listing) -> impl IntoView {
    let source = match listing.source.as_str() {
        "" => "duskcode".to_owned(),
        source => source.to_owned(),
    };
    let state = match (&listing.installed, &listing.update) {
        (Some(_), Some(update)) => format!("Installed; update to {update} in the desktop app"),
        (Some(version), None) => format!("Installed, {version}"),
        (None, _) => "Install it in the desktop app or the terminal".to_owned(),
    };
    let warnings = [
        listing
            .clash
            .then(|| "Another source offers the same name; check where it comes from.".to_owned()),
        listing.unfit.clone(),
    ];
    let permissions = (listing.kind == Kind::Plugin).then(|| {
        let lines = if listing.permissions.is_empty() {
            vec!["Nothing beyond its own folder".to_owned()]
        } else {
            listing.permissions.clone()
        };
        view! { <p class="menu-title">"It may"</p><ul class="plain">{lines.into_iter().map(|line| view! { <li>{line}</li> }).collect_view()}</ul> }
    });
    let pack = (listing.kind == Kind::Pack).then(|| {
        let opened = listing.name.clone();
        view! {
            <p class="menu-title">{format!("{} skills", listing.skills.len())}</p>
            <ul class="plain">{listing.skills.iter().map(|skill| {
                // The pack is the context, so a skill goes by its own name.
                let short = skill.strip_prefix(&format!("{}/", listing.name)).unwrap_or(skill).to_owned();
                view! { <li title=skill.clone()>{short}</li> }
            }).collect_view()}</ul>
            <button class="link" on:click=move |_| { shop.window.set(None); shop.open.set(Some(opened.clone())); }>"Open the pack's page"</button>
        }
    });
    let day = |text: &Option<String>| {
        text.as_deref()
            .and_then(|text| text.get(..10))
            .map(str::to_owned)
    };
    let dates = [
        ("Released", day(&listing.created)),
        ("Updated", day(&listing.updated)),
    ]
    .into_iter()
    .filter_map(|(label, date)| date.map(|date| view! { <dt>{label}</dt><dd>{date}</dd> }))
    .collect_view();
    let homepage = listing
        .homepage
        .clone()
        .filter(|url| url.starts_with("https://"))
        .map(|url| view! { <a class="link" href=url target="_blank" rel="noopener noreferrer"><Icon name=IconName::ExternalLink />"Its page"</a> });
    view! {
        <aside class="store-side">
            <span class="vendor">{format!("From {source}")}</span>
            {card::facts(listing)}
            {hero::tags(listing)}
            <p class="state">{state}</p>
            {warnings.into_iter().flatten().map(|text| view! { <p class="warn"><Icon name=IconName::TriangleAlert />{text}</p> }).collect_view()}
            {permissions}
            {pack}
            <dl class="dates">{dates}</dl>
            {homepage}
        </aside>
    }
}
