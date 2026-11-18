//! A listing as a card, as in the desktop app: the cover on top at 40:21 with nothing
//! drawn over it, the name and kind, two lines of text (a warning takes the first), and a
//! footer with the tier mark on the left and one slot on the right. Every card of a row
//! has the same height whatever its text. The browser installs nothing, so the slot holds
//! a state or says where installing happens. The tier mark is an icon, a colour and a
//! word, never colour alone. Listing text is drawn as text nodes only.

use dusk_protocol::{
    catalog::SkillPack,
    client::Switch as Flag,
    plugin::{Kind, Listing, Tier},
};
use leptos::prelude::*;

use super::{
    Shop,
    placeholder::{Picture, placeholder},
};
use crate::{
    api,
    ui::{Icon, IconName, Switch, run},
};

/// Why the slot has no Install button, for a screen reader.
const IN_APP: &str = ": install it in the desktop app or the terminal";

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

/// The tier's mark, the kind (with a pack's skill count), the version and, for a mirror
/// the user added, its repository's GitHub stars.
pub fn facts(listing: &Listing) -> AnyView {
    let kind = match (listing.kind, listing.skills.len()) {
        (Kind::Pack, count) if count > 0 => format!("Pack \u{b7} {count} skills"),
        (kind, _) => kind_word(kind).to_owned(),
    };
    view! {
        <span class="facts">
            {mark(listing.tier)}
            <span>{kind}</span>
            <span>{format!("v{}", listing.version)}</span>
            {listing.repo_stars.as_ref().map(|stars| view! { <span>{stars.label()}</span> })}
        </span>
    }
    .into_any()
}

/// What goes in a cover's frame: the picture, cropped to fill, or the placeholder.
pub fn art(kind: Kind, name: &str, picture: Picture) -> AnyView {
    match picture {
        Picture::Shown(url) => view! { <img src=url alt=format!("{name} cover") /> }.into_any(),
        other => placeholder(kind, &other),
    }
}

/// A listing's cover frame, redrawn alone when its picture arrives.
pub fn cover(shop: Shop, listing: &Listing) -> AnyView {
    let (kind, name) = (listing.kind, listing.name.clone());
    let sha = listing.cover.as_ref().map(|cover| cover.sha256.clone());
    view! { <div class="cover">{move || art(kind, &name, shop.picture(sha.as_deref()))}</div> }
        .into_any()
}

/// The footer's one slot: a state, or where installing happens. Never two things.
pub fn slot(listing: &Listing) -> AnyView {
    match (&listing.installed, &listing.update, &listing.unfit) {
        (Some(_), Some(version), _) => view! {
            <span class="slot-state waiting"><Icon name=IconName::ArrowUp />"Update waiting"
                <span class="sr">{format!(": {version}{IN_APP}")}</span></span>
        }
        .into_any(),
        (Some(_), None, _) => {
            view! { <span class="slot-state installed"><Icon name=IconName::Check />"Installed"</span> }
                .into_any()
        }
        (None, _, Some(_)) => view! { <span class="slot-note">"Cannot run here"</span> }.into_any(),
        (None, _, None) => {
            view! { <span class="slot-note">"In the app"<span class="sr">{IN_APP}</span></span> }
                .into_any()
        }
    }
}

/// One warning line above the text: a name another source also offers, else why it
/// cannot run here.
fn notice(listing: &Listing) -> Option<AnyView> {
    let text = match (listing.clash, &listing.unfit) {
        (true, _) => "Same name from another source".to_owned(),
        (false, Some(why)) => why.clone(),
        (false, None) => return None,
    };
    Some(
        view! { <p class="notice"><Icon name=IconName::TriangleAlert /><span>{text}</span></p> }
            .into_any(),
    )
}

/// The whole card opens the listing through its name, its one tab stop; anything in the
/// slot that can be pressed sits above it and keeps its own click.
pub fn capsule(shop: Shop, listing: Listing) -> AnyView {
    let notice = notice(&listing);
    let one = notice.is_some();
    let opened = listing.clone();
    view! {
        <article class="capsule">
            {cover(shop, &listing)}
            <div class="body">
                <div class="name">
                    <button class="open" title=listing.name.clone()
                        on:click=move |_| shop.open_listing(&opened)>{listing.name.clone()}</button>
                    <span class="kind">{kind_word(listing.kind)}</span>
                </div>
                <div class="text">{notice}<p class="desc" class:one=one>{listing.description.clone()}</p></div>
                <div class="foot">{mark(listing.tier)}<span class="grow"></span>{slot(&listing)}</div>
            </div>
        </article>
    }
    .into_any()
}

/// A pack here that no source lists, with its switch in the slot; the card opens its page.
pub fn pack(shop: Shop, pack: SkillPack) -> impl IntoView {
    let on = pack.skills.iter().filter(|skill| skill.enabled).count();
    let (opened, switched) = (pack.name.clone(), pack.name.clone());
    let origin = if pack.builtin { "Built-in" } else { "Local" };
    let title = if pack.title.is_empty() {
        pack.name.clone()
    } else {
        pack.title.clone()
    };
    let cover = art(Kind::Pack, &title, Picture::Missing);
    let tooltip = title.clone();
    view! {
        <article class="capsule">
            <div class="cover">{cover}</div>
            <div class="body">
                <div class="name">
                    <button class="open" title=tooltip
                        on:click=move |_| shop.open.set(Some(opened.clone()))>{title}</button>
                    <span class="kind">"Pack"</span>
                </div>
                <div class="text"><p class="desc">{format!("{on} of {} skills on", pack.skills.len())}</p></div>
                <div class="foot">
                    <span class="tier tier-local"><Icon name=IconName::TierLocal />{origin}</span>
                    <span class="grow"></span>
                    <span class="slot-control">
                        <Switch on=pack.enabled label="Pack" change=move |on| {
                            let name = switched.clone();
                            run(api::switch(Flag::SkillPack(name), on), move |()| shop.load_packs());
                        } />
                    </span>
                </div>
            </div>
        </article>
    }
}
