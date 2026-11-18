//! A pack's page, as a store page: its cover as a banner with the name, where it comes
//! from, its stars and its buttons; then its skills as rows with their descriptions and
//! switches on one column, once the pack is here, or by name while it is only offered.
//! A listing's window shows the same rows on its Skills tab.

use dusk_protocol::{
    catalog::SkillPack,
    client::Switch as CoreSwitch,
    plugin::{Kind, Listing, Tier},
};
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName,
        button::{Button, ButtonVariants},
        switch::Switch,
    },
    prelude::FluentBuilder,
    *,
};

use super::{Grid, actions, covers::Shown, hero, placeholder::Look, tier};
use crate::{
    ui::{Type, size, widgets},
    workspace::Workspace,
};

/// The longest description a row shapes; one line shows.
const DESCRIPTION_CHARS: usize = 200;

pub fn render(ws: &Workspace, name: &str, grid: Grid, cx: &mut Context<Workspace>) -> Div {
    let shop = &ws.workshop;
    let listing = shop
        .listings
        .iter()
        .flatten()
        .find(|listing| listing.kind == Kind::Pack && listing.name == name);
    let here = shop.packs.iter().flatten().find(|pack| pack.name == name);
    let back = Button::new("workshop-pack-back")
        .ghost()
        .icon(Icon::new(IconName::ChevronLeft))
        .label("Workshop")
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.workshop.pack = None;
            cx.notify();
        }));
    let page = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_XL))
        .w(px(grid.width))
        .child(div().flex().child(back))
        .child(banner(ws, name, listing, here, grid, cx));
    match (here, listing) {
        (Some(pack), _) => page.child(skills(ws, pack, cx)),
        (None, Some(listing)) => page.child(offered(listing, cx)),
        (None, None) => page.child(widgets::muted("This pack is no longer listed.", cx)),
    }
}

fn banner(
    ws: &Workspace,
    name: &str,
    listing: Option<&Listing>,
    here: Option<&SkillPack>,
    grid: Grid,
    cx: &mut Context<Workspace>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let title = here
        .map(|pack| pack.title.as_str())
        .filter(|title| !title.is_empty())
        .unwrap_or(name)
        .to_owned();
    let description = listing
        .map(|listing| listing.description.clone())
        .or_else(|| here.map(|pack| pack.description.clone()))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| match here.is_some_and(|pack| pack.builtin) {
            true => "The skills that come with duskcode.".to_owned(),
            false => "Skills in a folder of their own.".to_owned(),
        });
    let source = match listing.map(|listing| listing.source.as_str()) {
        Some("") | None if here.is_some_and(|pack| pack.builtin) => {
            "Comes with duskcode".to_owned()
        }
        Some("") | None => "Found on this machine".to_owned(),
        Some(source) => format!("From {source}"),
    };
    let facts = match listing {
        Some(listing) => hero::facts(listing, cx),
        None => div().flex().child(tier::mark(Tier::Local, cx)),
    };
    let panel = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_S))
        .min_w_0()
        .child(div().meta().text_color(muted).child(source))
        .child(div().display().truncate().child(title))
        .child(
            div()
                .body()
                .text_color(muted)
                .line_clamp(3)
                .text_ellipsis()
                .child(description),
        )
        .child(facts);
    let mut foot = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_S))
        .h(px(size::CONTROL));
    if let Some(listing) = listing {
        foot = foot.child(actions::slot(listing, ws, false, cx)).children(
            actions::remove(listing, ws, cx).map(|remove| div().pl(px(size::GAP_XL)).child(remove)),
        );
    }
    if let Some(pack) = here {
        let switched = pack.name.clone();
        foot = foot
            .child(div().flex_1())
            .child(div().small().text_color(muted).child("Pack"))
            .child(
                Switch::new("workshop-pack-switch")
                    .checked(pack.enabled)
                    .tooltip("Off hides all its skills")
                    .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                        ws.pack_switch(CoreSwitch::SkillPack(switched.clone()), *on, cx)
                    })),
            );
    }
    hero::banner(
        listing.map_or(Shown::Placeholder(Look::None), |listing| ws.cover(listing)),
        listing.map_or(Kind::Pack, |listing| listing.kind),
        grid.width,
        panel,
        foot,
        cx,
    )
}

/// The skills of the pack `listing` offers: those here with their switches, else by name.
pub fn skill_list(ws: &Workspace, listing: &Listing, cx: &mut Context<Workspace>) -> Div {
    let here = ws
        .workshop
        .packs
        .iter()
        .flatten()
        .find(|pack| pack.name == listing.name);
    match here {
        Some(pack) => skills(ws, pack, cx),
        None => offered(listing, cx),
    }
}

/// A skill's description as the skills shelf read it; none before it is here.
fn description<'a>(ws: &'a Workspace, name: &str) -> Option<&'a str> {
    ws.settings
        .as_ref()?
        .skills
        .iter()
        .find(|skill| skill.name == name)
        .map(|skill| skill.description.as_str())
        .filter(|text| !text.is_empty())
}

/// The skills of a pack that is here, a row each with its description and its switch at
/// the right edge.
fn skills(ws: &Workspace, pack: &SkillPack, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (border, muted, surface) = (theme.border, theme.muted_foreground, theme.muted);
    let mut rows = div()
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(border)
        .bg(surface)
        .overflow_hidden();
    for (index, skill) in pack.skills.iter().enumerate() {
        let full = skill.name.clone();
        let short = skill
            .name
            .strip_prefix(&format!("{}/", pack.name))
            .unwrap_or(&skill.name)
            .to_owned();
        let mut text = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .child(div().body().truncate().child(short));
        if let Some(about) = description(ws, &skill.name) {
            text = text.child(
                div()
                    .small()
                    .text_color(muted)
                    .truncate()
                    .child(super::capsule::clip(about, DESCRIPTION_CHARS)),
            );
        }
        rows = rows.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(size::GAP_M))
                .min_h(px(size::ROW))
                .py(px(size::GAP_S))
                .px(px(size::GAP_L))
                .when(index > 0, |row| row.border_t_1().border_color(border))
                .child(
                    Icon::new(IconName::BookOpen)
                        .size(px(size::ICON))
                        .text_color(muted),
                )
                .child(text)
                .child(
                    Switch::new(SharedString::from(format!("pack-skill-{}", skill.name)))
                        .checked(skill.enabled)
                        .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                            ws.pack_switch(CoreSwitch::Skill(full.clone()), *on, cx)
                        })),
                ),
        );
    }
    let on = pack.skills.iter().filter(|skill| skill.enabled).count();
    let mut section = div().flex().flex_col().gap(px(size::GAP_M)).child(
        div()
            .flex()
            .flex_row()
            .items_baseline()
            .gap(px(size::GAP_S))
            .child(
                div()
                    .body()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Skills"),
            )
            .child(
                div()
                    .meta()
                    .text_color(muted)
                    .child(format!("{on} of {} on", pack.skills.len())),
            ),
    );
    if !pack.enabled {
        section = section.child(
            div()
                .small()
                .text_color(muted)
                .child("The pack is off: its skills are hidden whatever their own switches say."),
        );
    }
    section.child(rows)
}

/// The skills a pack not yet here holds, by name.
fn offered(listing: &Listing, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (border, muted, surface) = (theme.border, theme.muted_foreground, theme.muted);
    // The pack is the context, so a skill goes by its own name.
    let prefix = format!("{}/", listing.name);
    let rows = listing.skills.iter().enumerate().map(|(index, skill)| {
        let short = skill.strip_prefix(&prefix).unwrap_or(skill).to_owned();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(size::GAP_M))
            .h(px(size::ROW))
            .px(px(size::GAP_L))
            .when(index > 0, |row| row.border_t_1().border_color(border))
            .child(
                Icon::new(IconName::BookOpen)
                    .size(px(size::ICON))
                    .text_color(muted),
            )
            .child(div().body().truncate().child(short))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_M))
        .child(
            div()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .child(widgets::count(listing.skills.len(), "skill")),
        )
        .child(
            div()
                .small()
                .text_color(muted)
                .child("Install the pack to switch its skills one by one."),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .rounded_lg()
                .border_1()
                .border_color(border)
                .bg(surface)
                .overflow_hidden()
                .children(rows),
        )
}
