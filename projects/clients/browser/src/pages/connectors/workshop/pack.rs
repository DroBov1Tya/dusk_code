//! A pack's page, as a store page: its cover as a banner with the name, where it comes
//! from and its facts, then its skills as rows with their switches on one column. A pack
//! not yet here lists its skills by name; it is installed in the desktop app or terminal.

use dusk_protocol::{
    client::Switch as Flag,
    plugin::{Kind, Tier},
};
use leptos::prelude::*;

use super::{Shop, card};
use crate::{
    api,
    ui::{Icon, IconName, Switch, run},
};

pub fn page(shop: Shop, name: String) -> impl IntoView {
    let listing = shop.list.with(|list| {
        list.as_ref().and_then(|list| {
            list.listings
                .iter()
                .find(|listing| listing.kind == Kind::Pack && listing.name == name)
                .cloned()
        })
    });
    let here = shop
        .packs
        .with(|packs| packs.iter().find(|pack| pack.name == name).cloned());
    let title = here
        .as_ref()
        .map(|pack| pack.title.clone())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| name.clone());
    let description = listing
        .as_ref()
        .map(|listing| listing.description.clone())
        .or_else(|| here.as_ref().map(|pack| pack.description.clone()))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "Skills in a folder of their own.".to_owned());
    let source = match listing.as_ref().map(|listing| listing.source.clone()) {
        Some(source) if !source.is_empty() => format!("From {source}"),
        _ if here.as_ref().is_some_and(|pack| pack.builtin) => "Comes with duskcode".to_owned(),
        _ => "Found on this machine".to_owned(),
    };
    let facts = match &listing {
        Some(listing) => card::facts(listing),
        None => card::mark(Tier::Local).into_any(),
    };
    let art = card::art(
        &name,
        listing.as_ref().map_or(Tier::Local, |listing| listing.tier),
        listing.as_ref().and_then(|listing| shop.cover(listing)),
    );
    let state = listing.as_ref().and_then(card::state);
    let body = match (here, listing) {
        (Some(pack), _) => {
            let switched = pack.name.clone();
            let prefix = format!("{}/", pack.name);
            let on = pack.skills.iter().filter(|skill| skill.enabled).count();
            let count = pack.skills.len();
            let rows = pack
                .skills
                .into_iter()
                .map(|skill| {
                    let short = skill.name.strip_prefix(&prefix).unwrap_or(&skill.name).to_owned();
                    let full = skill.name.clone();
                    view! {
                        <div class="skill-row"><span class="grow">{short}</span>
                            <Switch on=skill.enabled change=move |on| {
                                run(api::switch(Flag::Skill(full.clone()), on), move |()| shop.load_packs());
                            } /></div>
                    }
                })
                .collect_view();
            view! {
                <div class="shelf-head"><h3>"Skills"</h3><span class="muted">{format!("{on} of {count} on")}</span>
                    <span class="grow"></span>
                    <Switch on=pack.enabled label="Pack" change=move |on| {
                        run(api::switch(Flag::SkillPack(switched.clone()), on), move |()| shop.load_packs());
                    } /></div>
                <div class="skill-rows">{rows}</div>
            }
            .into_any()
        }
        (None, Some(listing)) => view! {
            <div class="shelf-head"><h3>{format!("{} skills", listing.skills.len())}</h3></div>
            <p class="note">"Install the pack in the desktop app or the terminal to switch its skills."</p>
            <div class="skill-rows">{listing.skills.into_iter().map(|skill| view! { <div class="skill-row">{skill}</div> }).collect_view()}</div>
        }
        .into_any(),
        (None, None) => view! { <p class="muted">"This pack is no longer listed."</p> }.into_any(),
    };
    view! {
        <div><button class="ghost" on:click=move |_| shop.open.set(None)><Icon name=IconName::ChevronLeft />"Workshop"</button></div>
        <div class="hero-card">
            <div class="hero-art">{art}</div>
            <div class="hero-panel">
                <span class="vendor">{source}</span>
                <h2>{title}</h2>
                <p class="muted">{description}</p>
                {facts}
                <div class="hero-foot">{state}</div>
            </div>
        </div>
        <section class="shelf">{body}</section>
    }
}
