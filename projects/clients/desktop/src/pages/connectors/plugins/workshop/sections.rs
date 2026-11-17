//! The listings in sections: one per tier while the list is in trust order, Official
//! first, then the skill packs here that no source lists, the built-in one among them.
//! In another order the core's order is kept as one section, each card with its mark.

use dusk_protocol::{
    catalog::SkillPack,
    client::Switch as CoreSwitch,
    plugin::{Kind, Listing, Order, Tier},
};
use gpui_kit::{
    component::{ActiveTheme, IconName, Sizable, switch::Switch},
    *,
};

use super::{card, filters, tier};
use crate::{
    pages::connectors::{grid, matches},
    ui::{Type, widgets},
    workspace::Workspace,
};

pub fn render(ws: &Workspace, shown: &[&Listing], query: &str, cx: &mut Context<Workspace>) -> Div {
    let shop = &ws.workshop;
    let mut page = div().flex().flex_col().gap_6();
    let mut any = false;
    if shop.order == Order::Trust {
        for each in tier::ALL {
            let listings: Vec<&Listing> = shown
                .iter()
                .copied()
                .filter(|listing| listing.tier == each)
                .collect();
            if listings.is_empty() {
                continue;
            }
            any = true;
            page = page.child(section(
                heading(
                    Some(each),
                    tier::word(each),
                    tier::about(each),
                    listings.len(),
                    cx,
                ),
                ws,
                &listings,
                cx,
            ));
        }
    } else if !shown.is_empty() {
        any = true;
        let title = filters::order_word(shop.order);
        page = page.child(section(
            heading(
                None,
                title,
                "In the order the store gives.",
                shown.len(),
                cx,
            ),
            ws,
            shown,
            cx,
        ));
    }
    let local = local_packs(ws, query);
    if !local.is_empty() {
        any = true;
        let mut cards = grid();
        for pack in &local {
            cards = cards.child(pack_card(pack, cx));
        }
        page = page.child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(heading(
                    Some(Tier::Local),
                    "Built-in and local packs",
                    "Skill packs here that no source lists; the built-in one comes with duskcode.",
                    local.len(),
                    cx,
                ))
                .child(cards),
        );
    }
    if !any {
        let (title, hint) = match shop.listings.as_ref().is_some_and(|all| all.is_empty()) {
            true if !shop.failed_sources.is_empty() => {
                ("No source could be read", "Try again when online.")
            }
            true => ("The store is empty", "Nothing is offered yet."),
            false => ("Nothing matches", "Try another word or filter."),
        };
        return page.child(crate::ui::empty(IconName::Inbox, title, hint, cx));
    }
    page
}

fn section(
    heading: Div,
    ws: &Workspace,
    listings: &[&Listing],
    cx: &mut Context<Workspace>,
) -> Div {
    let mut cards = grid();
    for listing in listings {
        cards = cards.child(card::render(ws, listing, cx));
    }
    div().flex().flex_col().gap_3().child(heading).child(cards)
}

/// A section's mark and name with its count, and one line on where it comes from.
fn heading(mark: Option<Tier>, title: &str, about: &str, count: usize, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut name = div().flex().flex_row().items_center().gap_2();
    if let Some(each) = mark {
        name = name.child(tier::icon(each, 18., cx));
    }
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            name.child(
                div()
                    .body()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title.to_owned()),
            )
            .child(div().meta().text_color(muted).child(count.to_string())),
        )
        .child(div().meta().text_color(muted).child(about.to_owned()))
}

/// Packs here that no listing of the store names, unless a filter leaves them out.
fn local_packs<'a>(ws: &'a Workspace, query: &str) -> Vec<&'a SkillPack> {
    let shop = &ws.workshop;
    let listed = |name: &str| {
        shop.listings
            .iter()
            .flatten()
            .any(|listing| listing.kind == Kind::Pack && listing.name == name)
    };
    let shown = shop.tier.is_none_or(|each| each == Tier::Local)
        && shop.kind.is_none_or(|kind| kind == Kind::Pack)
        && shop.category.is_none();
    if !shown {
        return Vec::new();
    }
    shop.packs
        .iter()
        .flatten()
        .filter(|pack| !listed(&pack.name))
        .filter(|pack| matches(query, &[&pack.name, &pack.title, &pack.description]))
        .collect()
}

/// A pack here: its name, how many of its skills are on, and its switch.
fn pack_card(pack: &SkillPack, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (border, muted, surface) = (theme.border, theme.muted_foreground, theme.muted);
    let title = match pack.title.as_str() {
        "" => pack.name.clone(),
        title => title.to_owned(),
    };
    let on = pack.skills.iter().filter(|skill| skill.enabled).count();
    let mark = match pack.builtin {
        true => div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .text_color(tier::color(Tier::Local, cx))
            .child(tier::icon(Tier::Local, 13., cx))
            .child(
                div()
                    .meta()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Built-in"),
            ),
        false => tier::mark(Tier::Local, cx),
    };
    let (opened, switched) = (pack.name.clone(), pack.name.clone());
    div()
        .id(SharedString::from(format!("workshop-pack-{}", pack.name)))
        .w(px(card::WIDTH))
        .max_w_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_xl()
        .border_1()
        .border_color(border)
        .bg(surface)
        .cursor_pointer()
        .hover(|card| card.border_color(muted.opacity(0.4)))
        .on_click(cx.listener(move |ws, _, _, cx| {
            ws.workshop.pack = Some(opened.clone());
            cx.notify();
        }))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(crate::pages::connectors::card::icon_slot(&pack.name, cx).size(px(44.)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .body()
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(title),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(mark)
                                .child(div().meta().text_color(muted).child(format!(
                                    "{on} of {} on",
                                    widgets::count(pack.skills.len(), "skill")
                                ))),
                        ),
                )
                .child(
                    div()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            Switch::new(SharedString::from(format!("pack-{}", pack.name)))
                                .checked(pack.enabled)
                                .small()
                                .tooltip("The pack's switch: off hides all its skills")
                                .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                                    ws.pack_switch(CoreSwitch::SkillPack(switched.clone()), *on, cx)
                                })),
                        ),
                ),
        )
        .child(
            div()
                .small()
                .text_color(muted)
                .line_clamp(2)
                .text_ellipsis()
                .child(match pack.description.as_str() {
                    "" if pack.builtin => "The skills that come with duskcode.".to_owned(),
                    "" => "Skills in a folder of their own.".to_owned(),
                    text => text.to_owned(),
                }),
        )
}
