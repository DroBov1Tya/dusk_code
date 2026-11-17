//! A pack's page: where it comes from, its switch and its skills with theirs. A pack not
//! yet here lists its skills by name; it is installed in the desktop app or terminal.

use dusk_protocol::{client::Switch as Flag, plugin::Kind};
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
        .unwrap_or_default();
    let facts = match (&listing, &here) {
        (Some(listing), _) => card::facts(listing).into_any(),
        (None, Some(pack)) if pack.builtin => view! {
            <span class="tier tier-local"><Icon name=IconName::TierLocal />"Built-in"</span>
        }
        .into_any(),
        (None, _) => card::mark(dusk_protocol::plugin::Tier::Local).into_any(),
    };
    let body = match (here, listing) {
        (Some(pack), _) => {
            let switched = pack.name.clone();
            let prefix = format!("{}/", pack.name);
            let rows = pack
                .skills
                .into_iter()
                .map(|skill| {
                    let short = skill.name.strip_prefix(&prefix).unwrap_or(&skill.name).to_owned();
                    let full = skill.name.clone();
                    view! {
                        <Switch on=skill.enabled label=short change=move |on| {
                            run(api::switch(Flag::Skill(full.clone()), on), move |()| shop.load_packs());
                        } />
                    }
                })
                .collect_view();
            view! {
                <Switch on=pack.enabled label="The pack: off hides all its skills" change=move |on| {
                    run(api::switch(Flag::SkillPack(switched.clone()), on), move |()| shop.load_packs());
                } />
                <div class="card-tile">{rows}</div>
            }
            .into_any()
        }
        (None, Some(listing)) => view! {
            <p class="note">"Install the pack in the desktop app or the terminal to switch its skills."</p>
            <ul>{listing.skills.into_iter().map(|skill| view! { <li>{skill}</li> }).collect_view()}</ul>
        }
        .into_any(),
        (None, None) => view! { <p class="muted">"This pack is no longer listed."</p> }.into_any(),
    };
    view! {
        <div>
            <button class="ghost" on:click=move |_| shop.open.set(None)>"\u{2039} Workshop"</button>
        </div>
        <h2>{title}</h2>
        <div class="facts">{facts}</div>
        <p class="muted">{description}</p>
        {body}
    }
}
