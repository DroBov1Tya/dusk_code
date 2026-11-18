//! A listing as a capsule: its cover on top at the art's aspect, then its name and
//! stars, one line of what it is, and its tier mark with its one button on a row of a
//! control's height. The corner of the cover says Installed or Update; the other corner
//! warns when another source offers the same name. A click opens the listing's window.

use dusk_protocol::{
    catalog::SkillPack,
    client::Switch as CoreSwitch,
    plugin::{Listing, Tier},
};
use gpui_kit::{
    component::{ActiveTheme, Icon, IconName, switch::Switch},
    prelude::FluentBuilder,
    *,
};

use super::{actions, grid, tier};
use crate::{
    ui::{Type, size, widgets},
    workspace::Workspace,
};

/// How much a cover grows under the pointer: enough to answer it, not to move the page.
const ZOOM: f32 = 1.03;

pub fn render(
    ws: &Workspace,
    listing: &Listing,
    width: f32,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    let group = actions::id("capsule", listing);
    let mut top = div().relative().child(art(
        ws.workshop.covers.get(listing),
        &listing.name,
        listing.tier,
        width,
        Some(&group),
        cx,
    ));
    if let Some(badge) = state_badge(listing, cx) {
        top = top.child(badge.absolute().top(px(size::GAP_S)).right(px(size::GAP_S)));
    }
    if listing.clash {
        top = top.child(
            corner(
                IconName::TriangleAlert,
                "Same name elsewhere",
                cx.theme().warning,
                cx,
            )
            .absolute()
            .top(px(size::GAP_S))
            .left(px(size::GAP_S)),
        );
    }
    let failed = ws.workshop.failed.get(&listing.name).cloned();
    let line = match (failed, &listing.unfit) {
        (Some(error), _) => div().text_color(cx.theme().danger).child(error),
        (None, Some(why)) => div().text_color(cx.theme().warning).child(why.clone()),
        (None, None) => div().text_color(muted).child(listing.description.clone()),
    };
    let footer = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_S))
        .h(px(size::CONTROL))
        .child(tier::mark(listing.tier, cx))
        .child(div().flex_1())
        // The button keeps its click, so it does not open the window too.
        .children(actions::primary(listing, ws, true, cx).map(|button| {
            div()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(button)
        }));
    let opened = listing.clone();
    shell(group, cx)
        .w(px(width))
        .on_click(cx.listener(move |ws, _, window, cx| ws.open_listing(&opened, window, cx)))
        .child(top)
        .child(
            body()
                .child(name_row(&listing.name, listing.stars, cx))
                .child(line.small().truncate())
                .child(footer),
        )
}

/// A pack here that no source lists: a letter cover, how many of its skills are on, and
/// its switch where a listing has its button. A click opens its page.
pub fn pack(pack: &SkillPack, width: f32, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    let title = match pack.title.as_str() {
        "" => pack.name.clone(),
        title => title.to_owned(),
    };
    let on = pack.skills.iter().filter(|skill| skill.enabled).count();
    let word = if pack.builtin { "Built-in" } else { "Local" };
    let group = SharedString::from(format!("workshop-pack-{}", pack.name));
    let (opened, switched) = (pack.name.clone(), pack.name.clone());
    shell(group.clone(), cx)
        .w(px(width))
        .on_click(cx.listener(move |ws, _, _, cx| {
            ws.workshop.pack = Some(opened.clone());
            cx.notify();
        }))
        .child(art(None, &pack.name, Tier::Local, width, Some(&group), cx))
        .child(
            body()
                .child(name_row(&title, 0, cx))
                .child(div().small().truncate().text_color(muted).child(format!(
                    "{on} of {} on",
                    widgets::count(pack.skills.len(), "skill")
                )))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(size::GAP_S))
                        .h(px(size::CONTROL))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(size::GAP_XS))
                                .text_color(tier::color(Tier::Local, cx))
                                .child(tier::icon(Tier::Local, size::ICON, cx))
                                .child(div().meta().font_weight(FontWeight::MEDIUM).child(word)),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(
                                    Switch::new(SharedString::from(format!("pack-{}", pack.name)))
                                        .checked(pack.enabled)
                                        .tooltip("The pack's switch: off hides all its skills")
                                        .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                                            ws.pack_switch(
                                                CoreSwitch::SkillPack(switched.clone()),
                                                *on,
                                                cx,
                                            )
                                        })),
                                ),
                        ),
                ),
        )
}

/// The capsule's frame: it lifts under the pointer, darkens while pressed and shows the
/// accent ring while the keyboard is on it.
fn shell(group: SharedString, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (border, surface, hover, ring) = (theme.border, theme.muted, theme.list_hover, theme.ring);
    div()
        .id(group.clone())
        .group(group)
        .flex_none()
        .flex()
        .flex_col()
        .rounded_lg()
        .overflow_hidden()
        .border_1()
        .border_color(border)
        .bg(surface)
        .cursor_pointer()
        .hover(move |capsule| capsule.border_color(ring.opacity(0.45)).bg(hover))
        .active(move |capsule| capsule.bg(surface))
        .tab_stop(true)
        .tab_index(0)
        .focus_visible(move |capsule| capsule.border_color(ring))
}

fn body() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_XS))
        .px(px(size::GAP_M))
        .pt(px(size::GAP_M))
        .pb(px(size::GAP_S))
}

/// The name, cut when long, with the stars at the right end.
fn name_row(name: &str, stars: u32, cx: &App) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_S))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .truncate()
                .child(name.to_owned()),
        )
        .when(stars > 0, |row| row.child(actions::stars(stars, cx)))
}

/// The cover at `width` and the art's aspect, growing a little under the pointer over
/// `group`; without one, the name's letter on the tier's tint.
pub fn art(
    image: Option<std::sync::Arc<Image>>,
    name: &str,
    tier: Tier,
    width: f32,
    group: Option<&SharedString>,
    cx: &App,
) -> Div {
    let height = grid::cover_height(width);
    let frame = div()
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(height))
        .overflow_hidden()
        .bg(cx.theme().accent);
    let picture = match image {
        Some(image) => div()
            .size_full()
            .child(img(image).size_full().object_fit(ObjectFit::Cover)),
        None => {
            let letter = name
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "?".to_owned());
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(size::GAP_S))
                .bg(tier::color(tier, cx).opacity(0.12))
                .child(
                    div()
                        .display()
                        .text_color(cx.theme().foreground.opacity(0.8))
                        .child(letter),
                )
        }
    };
    let (grown_w, grown_h) = (width * ZOOM, height * ZOOM);
    let layer = div()
        .absolute()
        .top_0()
        .left_0()
        .w(px(width))
        .h(px(height))
        .child(picture);
    let layer = match group {
        Some(group) => layer.group_hover(group.clone(), move |style| {
            style
                .w(px(grown_w))
                .h(px(grown_h))
                .left(px((width - grown_w) / 2.))
                .top(px((height - grown_h) / 2.))
        }),
        None => layer,
    };
    frame.child(layer)
}

/// Installed, or an update waiting: a badge on the cover's corner.
pub fn state_badge(listing: &Listing, cx: &App) -> Option<Div> {
    let theme = cx.theme();
    match (&listing.installed, &listing.update) {
        (Some(_), Some(_)) => Some(corner(IconName::ArrowUp, "Update", theme.primary, cx)),
        (Some(_), None) => Some(corner(IconName::Check, "Installed", theme.success, cx)),
        (None, _) => None,
    }
}

/// A short word with its icon on the cover, on the page's own colour so it reads on any art.
fn corner(icon: IconName, text: &'static str, color: Hsla, cx: &App) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_XS))
        .h(px(size::GAP_XL))
        .px(px(size::GAP_S))
        .rounded_md()
        .bg(cx.theme().background.opacity(0.9))
        .text_color(color)
        .meta()
        .font_weight(FontWeight::MEDIUM)
        .child(Icon::new(icon).size(px(size::ICON)).text_color(color))
        .child(text)
}
