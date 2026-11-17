//! A pack's page: what it is and where it comes from, its switch and its install or
//! update, and its skills, each with its own switch once the pack is here.

use dusk_protocol::{
    catalog::SkillPack,
    client::Switch as CoreSwitch,
    plugin::{Kind, Listing, Tier},
};
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
        switch::Switch,
    },
    *,
};

use super::{card, tier};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub fn render(ws: &Workspace, name: &str, cx: &mut Context<Workspace>) -> Div {
    let shop = &ws.workshop;
    let listing = shop
        .listings
        .iter()
        .flatten()
        .find(|listing| listing.kind == Kind::Pack && listing.name == name);
    let here = shop.packs.iter().flatten().find(|pack| pack.name == name);
    let back = Button::new("workshop-pack-back")
        .ghost()
        .small()
        .icon(Icon::new(IconName::ChevronLeft))
        .label("Workshop")
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.workshop.pack = None;
            cx.notify();
        }));
    let mut page = div()
        .flex()
        .flex_col()
        .gap_5()
        .max_w(px(880.))
        .child(div().flex().child(back))
        .child(header(ws, name, listing, here, cx));
    page = match (here, listing) {
        (Some(pack), _) => page.child(skills(pack, cx)),
        (None, Some(listing)) => page.child(offered(listing, cx)),
        (None, None) => page.child(widgets::muted("This pack is no longer listed.", cx)),
    };
    page
}

fn header(
    ws: &Workspace,
    name: &str,
    listing: Option<&Listing>,
    here: Option<&SkillPack>,
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
        .unwrap_or_default();
    let thumb = match listing {
        Some(listing) => card::thumb(ws, listing, 64., cx),
        None => crate::pages::connectors::card::icon_slot(name, cx).size(px(64.)),
    };
    let facts = match (listing, here) {
        (Some(listing), _) => card::facts(listing, cx),
        (None, Some(pack)) if pack.builtin => div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .text_color(tier::color(Tier::Local, cx))
            .child(tier::icon(Tier::Local, 13., cx))
            .child(div().meta().child("Built-in")),
        (None, _) => tier::mark(Tier::Local, cx),
    };
    let mut side = div().flex().flex_row().items_center().gap_2();
    if let Some(pack) = here {
        let switched = pack.name.clone();
        side = side
            .child(div().meta().text_color(muted).child("Pack"))
            .child(
                Switch::new("workshop-pack-switch")
                    .checked(pack.enabled)
                    .tooltip("Off hides all its skills")
                    .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                        ws.pack_switch(CoreSwitch::SkillPack(switched.clone()), *on, cx)
                    })),
            );
    }
    if let Some(listing) = listing {
        let busy = ws.workshop.busy.get(&listing.name).copied();
        side = side.children(card::actions(listing, busy, cx));
    }
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap_4()
        .child(thumb)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(rems(1.6))
                        .font_weight(FontWeight::BOLD)
                        .child(title),
                )
                .child(facts)
                .child(div().body().text_color(muted).child(description)),
        )
        .child(side)
}

/// The skills of a pack that is here, each with its switch.
fn skills(pack: &SkillPack, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (border, muted) = (theme.border, theme.muted_foreground);
    let mut rows = widgets::card(cx).gap_0().p_0();
    for (index, skill) in pack.skills.iter().enumerate() {
        let full = skill.name.clone();
        let short = skill
            .name
            .strip_prefix(&format!("{}/", pack.name))
            .unwrap_or(&skill.name)
            .to_owned();
        let row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_4()
            .py_2p5()
            .child(div().flex_1().min_w_0().body().truncate().child(short))
            .child(
                Switch::new(SharedString::from(format!("pack-skill-{}", skill.name)))
                    .checked(skill.enabled)
                    .small()
                    .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                        ws.pack_switch(CoreSwitch::Skill(full.clone()), *on, cx)
                    })),
            );
        rows = rows.child(if index > 0 {
            row.border_t_1().border_color(border)
        } else {
            row
        });
    }
    let mut section = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(crate::ui::group(widgets::count(pack.skills.len(), "skill"), cx).px_0());
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
    let muted = cx.theme().muted_foreground;
    let mut list = widgets::card(cx).gap_1();
    for skill in &listing.skills {
        list = list.child(div().body().child(skill.clone()));
    }
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(crate::ui::group(widgets::count(listing.skills.len(), "skill"), cx).px_0())
        .child(
            div()
                .small()
                .text_color(muted)
                .child("Install the pack to switch its skills one by one."),
        )
        .child(list)
}
