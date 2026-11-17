//! A listing or a pack as a card, and the tier mark: the same icon, colour and word as in
//! the desktop app, never colour alone.

use dusk_protocol::{
    catalog::SkillPack,
    client::Switch as Flag,
    plugin::{Kind, Listing, Tier},
};
use leptos::prelude::*;

use super::Shop;
use crate::{
    api,
    ui::{Icon, IconName, Switch, run},
};

pub fn word(tier: Tier) -> &'static str {
    match tier {
        Tier::Official => "Official",
        Tier::Verified => "Verified",
        Tier::Community => "Community",
        Tier::Local => "Local",
    }
}

pub fn class(tier: Tier) -> &'static str {
    match tier {
        Tier::Official => "tier-official",
        Tier::Verified => "tier-verified",
        Tier::Community => "tier-community",
        Tier::Local => "tier-local",
    }
}

pub fn icon(tier: Tier) -> IconName {
    match tier {
        Tier::Official => IconName::TierOfficial,
        Tier::Verified => IconName::TierVerified,
        Tier::Community => IconName::TierCommunity,
        Tier::Local => IconName::TierLocal,
    }
}

pub fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Plugin => "Plugin",
        Kind::Skill => "Skill",
        Kind::Pack => "Pack",
        Kind::Agent => "Agent",
    }
}

pub fn mark(tier: Tier) -> impl IntoView {
    view! { <span class=format!("tier {}", class(tier))><Icon name=icon(tier) />{word(tier)}</span> }
}

/// The tier's mark, the kind, the stars and, for a pack, how many skills.
pub fn facts(listing: &Listing) -> AnyView {
    let stars = (listing.stars > 0).then(|| format!("\u{2605} {}", listing.stars));
    let skills = (listing.kind == Kind::Pack).then(|| format!("{} skills", listing.skills.len()));
    view! {
        <span class="facts">
            {mark(listing.tier)}
            <span>{kind_word(listing.kind)}</span>
            {stars.map(|text| view! { <span aria-label="stars">{text}</span> })}
            {skills.map(|text| view! { <span>{text}</span> })}
        </span>
    }
    .into_any()
}

fn thumb(name: &str, cover: Option<String>) -> AnyView {
    match cover {
        Some(url) => view! { <img class="thumb" src=url alt="" /> }.into_any(),
        None => {
            let letter = name
                .chars()
                .find(|c| c.is_alphanumeric())
                .map_or("?".to_owned(), |c| c.to_uppercase().to_string());
            view! { <span class="thumb" aria-hidden="true">{letter}</span> }.into_any()
        }
    }
}

pub fn listing(shop: Shop, listing: Listing) -> AnyView {
    let pack = listing.kind == Kind::Pack;
    let opened = listing.name.clone();
    let state = match (&listing.installed, &listing.update) {
        (Some(_), Some(update)) => {
            Some(format!("Installed; update to {update} in the desktop app"))
        }
        (Some(_), None) => Some("Installed".to_owned()),
        (None, _) => None,
    };
    let cover = shop.cover(&listing);
    view! {
        <div class="card-tile listing" class:openable=pack
            role=pack.then_some("button")
            on:click=move |_| if pack { shop.open.set(Some(opened.clone())) }>
            <div class="top">
                {thumb(&listing.name, cover)}
                <div>
                    <div class="tile-head"><b>{listing.name.clone()}</b><span class="muted">{listing.version.clone()}</span></div>
                    {facts(&listing)}
                </div>
            </div>
            <p class="muted">{listing.description.clone()}</p>
            {listing.clash.then(|| view! { <p class="warn">"Another source offers the same name"</p> })}
            {listing.unfit.clone().map(|why| view! { <p class="warn">{why}</p> })}
            {state.map(|text| view! { <p class="muted">{text}</p> })}
        </div>
    }
    .into_any()
}

/// A pack here that no source lists, with its switch.
pub fn pack(shop: Shop, pack: SkillPack) -> impl IntoView {
    let on = pack.skills.iter().filter(|skill| skill.enabled).count();
    let (opened, switched) = (pack.name.clone(), pack.name.clone());
    let origin = if pack.builtin { "Built-in" } else { "Local" };
    let title = if pack.title.is_empty() {
        pack.name.clone()
    } else {
        pack.title.clone()
    };
    view! {
        <div class="card-tile listing openable" role="button"
            on:click=move |_| shop.open.set(Some(opened.clone()))>
            <div class="top">
                {thumb(&pack.name, None)}
                <div>
                    <div class="tile-head"><b>{title}</b></div>
                    <span class="facts">
                        <span class="tier tier-local"><Icon name=IconName::TierLocal />{origin}</span>
                        <span>{format!("{on} of {} skills on", pack.skills.len())}</span>
                    </span>
                </div>
            </div>
            <div on:click=|ev| ev.stop_propagation()>
                <Switch on=pack.enabled label="Pack" change=move |on| {
                    let name = switched.clone();
                    run(api::switch(Flag::SkillPack(name), on), move |()| shop.load_packs());
                } />
            </div>
        </div>
    }
}
