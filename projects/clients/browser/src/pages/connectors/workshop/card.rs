//! A listing as a capsule, as in the desktop app: its cover on top at the art's aspect,
//! its name and stars, one line of what it is, its tier mark and its state; Installed
//! or Update on the cover's corner, a warning on the other when another source offers
//! the same name. The tier mark is the same icon, colour and word, never colour alone.

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

/// The tier's mark, the kind, the stars and the version.
pub fn facts(listing: &Listing) -> AnyView {
    view! {
        <span class="facts">
            {mark(listing.tier)}
            <span>{kind_word(listing.kind)}</span>
            {(listing.stars > 0).then(|| stars(listing.stars))}
            <span>{format!("v{}", listing.version)}</span>
        </span>
    }
    .into_any()
}

pub fn stars(count: u32) -> impl IntoView {
    view! { <span class="stars" aria-label=format!("{count} stars")><Icon name=IconName::Star />{count}</span> }
}

/// The cover at the art's aspect, or the name's letter on the tier's tint.
pub fn art(name: &str, tier: Tier, cover: Option<String>) -> AnyView {
    match cover {
        Some(url) => view! { <div class="art"><img src=url alt="" /></div> }.into_any(),
        None => {
            let letter = name
                .chars()
                .find(|c| c.is_alphanumeric())
                .map_or("?".to_owned(), |c| c.to_uppercase().to_string());
            view! { <div class=format!("art letter {}", class(tier)) aria-hidden="true"><span>{letter}</span></div> }
                .into_any()
        }
    }
}

/// Installed, or an update waiting, on the cover's corner.
pub fn state(listing: &Listing) -> Option<AnyView> {
    match (&listing.installed, &listing.update) {
        (Some(_), Some(_)) => Some(view! { <span class="corner update"><Icon name=IconName::ArrowUp />"Update"</span> }.into_any()),
        (Some(_), None) => Some(view! { <span class="corner installed"><Icon name=IconName::Check />"Installed"</span> }.into_any()),
        (None, _) => None,
    }
}

pub fn capsule(shop: Shop, listing: Listing) -> AnyView {
    let cover = shop.cover(&listing);
    let line = match &listing.unfit {
        Some(why) => view! { <p class="line warn">{why.clone()}</p> }.into_any(),
        None => view! { <p class="line muted">{listing.description.clone()}</p> }.into_any(),
    };
    let opened = listing.clone();
    view! {
        <button class="capsule" on:click=move |_| shop.open_listing(&opened)
            aria-label=format!("{}, {}", listing.name, word(listing.tier))>
            <div class="cover">
                {art(&listing.name, listing.tier, cover)}
                {state(&listing)}
                {listing.clash.then(|| view! { <span class="corner clash"><Icon name=IconName::TriangleAlert />"Same name elsewhere"</span> })}
            </div>
            <div class="body">
                <div class="name"><b>{listing.name.clone()}</b>{(listing.stars > 0).then(|| stars(listing.stars))}</div>
                {line}
                <div class="foot">{mark(listing.tier)}<span class="muted">{kind_word(listing.kind)}</span></div>
            </div>
        </button>
    }
    .into_any()
}

/// A pack here that no source lists, with its switch; a click opens its page.
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
        <div class="capsule" role="button" tabindex="0"
            on:click=move |_| shop.open.set(Some(opened.clone()))>
            <div class="cover">{art(&pack.name, Tier::Local, None)}</div>
            <div class="body">
                <div class="name"><b>{title}</b></div>
                <p class="line muted">{format!("{on} of {} skills on", pack.skills.len())}</p>
                <div class="foot">
                    <span class="tier tier-local"><Icon name=IconName::TierLocal />{origin}</span>
                    <span class="grow"></span>
                    <span on:click=|ev| ev.stop_propagation()>
                        <Switch on=pack.enabled label="Pack" change=move |on| {
                            let name = switched.clone();
                            run(api::switch(Flag::SkillPack(name), on), move |()| shop.load_packs());
                        } />
                    </span>
                </div>
            </div>
        </div>
    }
}
