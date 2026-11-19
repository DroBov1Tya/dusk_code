//! A listing as a card: its cover at the card's inner width, then its name and kind, its
//! byline, two lines of what it is (a notice takes the first when there is one), and a footer
//! with the tier mark at the left and one slot at the right holding a button or a state.
//! Nothing is drawn over the cover, and every card of a row is the same height whatever
//! its text. A click opens the listing's window.

use crate::ui::elevation::{Elevation, Level};
use dusk_protocol::{
    catalog::SkillPack,
    client::Switch as CoreSwitch,
    plugin::{Kind, Listing, Tier},
};
use gpui_kit::{
    component::{ActiveTheme, Icon, IconName, switch::Switch, tooltip::Tooltip},
    prelude::FluentBuilder,
    *,
};

use super::{
    actions,
    covers::Shown,
    grid,
    placeholder::{self, Look},
    tier,
};
use crate::{
    ui::{Type, size, widgets},
    workspace::Workspace,
};

/// The card's corner; the cover inside its 1 px border takes one less.
const RADIUS: f32 = 12.;
const BORDER: f32 = 1.;
const NAME: f32 = 22.;
const BYLINE: f32 = 16.;
/// One line of the text; the text is two.
const LINE: f32 = 20.;
const NOTICE_ICON: f32 = 14.;
/// More than two lines never show; the rest is not shaped at all.
const TEXT_CHARS: usize = 280;

/// A card's height at `width`: the same for every card of a row.
pub fn height(width: f32) -> f32 {
    let cover = grid::cover_height(inner(width));
    let body = size::GAP_M
        + NAME
        + size::GAP_XS
        + BYLINE
        + size::GAP_XS
        + 2. * LINE
        + size::GAP_XS
        + size::CONTROL;
    cover + body + size::GAP_S + 2. * BORDER
}

/// The width inside the card's border, which the cover fills.
fn inner(width: f32) -> f32 {
    width - 2. * BORDER
}

pub fn render(
    ws: &Workspace,
    listing: &Listing,
    width: f32,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (warning, danger) = (theme.warning, theme.danger);
    let group = actions::id("capsule", listing);
    let failed = ws.workshop.failed.get(&listing.name);
    let notice = match (failed, &listing.unfit) {
        (Some(error), _) => Some((error.clone(), danger)),
        (None, Some(why)) => Some((why.clone(), warning)),
        (None, None) if listing.clash => {
            Some(("Same name from another source".to_owned(), warning))
        }
        (None, None) => None,
    };
    // The button keeps its click, so it does not open the window too.
    let slot = div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(actions::slot(listing, ws, true, cx));
    let opened = listing.clone();
    shell(group, width, cx)
        .on_click(cx.listener(move |ws, _, window, cx| ws.open_listing(&opened, window, cx)))
        .child(art(
            ws.cover(listing),
            &listing.name,
            listing.kind,
            inner(width),
            top_corners(),
            cx,
        ))
        .child(
            body()
                .child(name_row(listing, cx))
                .child(byline_row(&super::byline::byline(listing), cx))
                .child(text(&listing.description, notice, cx))
                .child(footer(tier::mark(listing.tier, cx), slot)),
        )
}

/// A pack here that no source lists: how many of its skills are on, and its switch in the
/// slot where a listing has its button. A click opens its page.
pub fn pack(pack: &SkillPack, width: f32, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let title = match pack.title.as_str() {
        "" => pack.name.clone(),
        title => title.to_owned(),
    };
    let on = pack.skills.iter().filter(|skill| skill.enabled).count();
    let mut about = format!("{on} of {} on.", widgets::count(pack.skills.len(), "skill"));
    if !pack.description.is_empty() {
        about = format!("{about} {}", pack.description);
    }
    let word = if pack.builtin { "Built-in" } else { "Local" };
    let group = SharedString::from(format!("workshop-pack-{}", pack.name));
    let (opened, switched) = (pack.name.clone(), pack.name.clone());
    let mark = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_XS))
        .text_color(tier::color(Tier::Local, cx))
        .child(tier::icon(Tier::Local, size::ICON, cx))
        .child(div().meta().font_weight(FontWeight::MEDIUM).child(word));
    let switch = div()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            Switch::new(SharedString::from(format!("pack-{}", pack.name)))
                .checked(pack.enabled)
                .tooltip("The pack's switch: off hides all its skills")
                .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                    ws.pack_switch(CoreSwitch::SkillPack(switched.clone()), *on, cx)
                })),
        );
    shell(group, width, cx)
        .on_click(cx.listener(move |ws, _, _, cx| {
            ws.workshop.pack = Some(opened.clone());
            cx.notify();
        }))
        .child(art(
            Shown::Placeholder(Look::None),
            &pack.name,
            Kind::Pack,
            inner(width),
            top_corners(),
            cx,
        ))
        .child(
            body()
                .child(name_line(
                    SharedString::from(format!("pack-name-{}", pack.name)),
                    &title,
                    Kind::Pack,
                    cx,
                ))
                .child(byline_row(&pack_byline(pack), cx))
                .child(text(&about, None, cx))
                .child(footer(mark, switch)),
        )
}

/// The card's frame: a step lighter with an accent border under the pointer, back to its
/// surface while pressed, the accent border while the keyboard is on it.
fn shell(group: SharedString, width: f32, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (surface, hover, ring) = (theme.muted, theme.list_hover, theme.ring);
    let dent = crate::ui::press::dent(cx);
    div()
        .id(group.clone())
        .group(group)
        .flex_none()
        .flex()
        .flex_col()
        .w(px(width))
        .h(px(height(width)))
        .rounded(px(RADIUS))
        .elevation(Level::Raised, cx)
        .cursor_pointer()
        .hover(move |card| card.border_color(ring.opacity(0.45)).bg(hover))
        .active(move |card| {
            card.border_color(ring.opacity(0.6))
                .bg(surface)
                .shadow(dent)
        })
        .tab_stop(true)
        .tab_index(0)
        .focus_visible(move |card| card.border_color(ring))
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

fn name_row(listing: &Listing, cx: &App) -> Stateful<Div> {
    name_line(
        actions::id("name", listing),
        &listing.name,
        listing.kind,
        cx,
    )
}

/// The name on one line, whole in its tooltip; the kind at the right.
fn name_line(id: SharedString, name: &str, kind: Kind, cx: &App) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    let tip = SharedString::from(name.to_owned());
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_S))
        .h(px(NAME))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .truncate()
                .child(tip.clone()),
        )
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(
            div()
                .flex_none()
                .meta()
                .text_color(muted)
                .child(super::kind_word(kind)),
        )
}

/// The byline on one muted line, cut at the card's edge; the row keeps its height when
/// the list gives nothing for it.
fn byline_row(text: &str, cx: &App) -> Div {
    div()
        .h(px(BYLINE))
        .meta()
        .text_color(cx.theme().muted_foreground)
        .truncate()
        .child(text.to_owned())
}

/// A pack here that no source lists: the built-in pack is duskcode's, any other is only
/// counted.
fn pack_byline(pack: &SkillPack) -> String {
    let count = widgets::count(pack.skills.len(), "skill");
    match pack.builtin {
        true => format!("by duskcode \u{b7} {count}"),
        false => count,
    }
}

/// Two lines of the description; a notice takes the first, in its colour.
fn text(description: &str, notice: Option<(String, Hsla)>, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    let lines = if notice.is_some() { 1 } else { 2 };
    let about = div()
        .w_full()
        .small()
        .line_height(px(LINE))
        .text_color(muted)
        .line_clamp(lines)
        .text_ellipsis()
        .child(clip(description, TEXT_CHARS));
    div()
        .flex()
        .flex_col()
        .h(px(2. * LINE))
        .overflow_hidden()
        .when_some(notice, |text, (notice, color)| {
            text.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(size::GAP_XS))
                    .h(px(LINE))
                    .small()
                    .text_color(color)
                    .child(
                        Icon::new(IconName::TriangleAlert)
                            .size(px(NOTICE_ICON))
                            .text_color(color),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(clip(&notice, TEXT_CHARS)),
                    ),
            )
        })
        .child(about)
}

/// The tier mark at the left, the one slot at the right.
fn footer(mark: Div, slot: Div) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_S))
        .mt(px(size::GAP_XS))
        .h(px(size::CONTROL))
        .child(mark)
        .child(div().flex_1())
        .child(slot)
}

/// A third party's text as one plain line of at most `max` characters: line breaks and
/// controls become spaces, runs of space one.
pub fn clip(text: &str, max: usize) -> String {
    let mut line = String::with_capacity(text.len().min(max * 4));
    let mut count = 0;
    let mut space = false;
    for c in text.trim().chars() {
        if c.is_whitespace() || c.is_control() {
            space = true;
            continue;
        }
        if count >= max {
            line.push('\u{2026}');
            break;
        }
        if space && !line.is_empty() {
            line.push(' ');
            count += 1;
        }
        space = false;
        line.push(c);
        count += 1;
    }
    line
}

/// The top corners of a cover inside a card's border.
pub fn top_corners() -> Corners<Pixels> {
    let radius = px(RADIUS - BORDER);
    Corners {
        top_left: radius,
        top_right: radius,
        bottom_right: px(0.),
        bottom_left: px(0.),
    }
}

/// Rounds an element's corners one by one.
pub fn rounded<E: Styled>(element: E, corners: Corners<Pixels>) -> E {
    element
        .rounded_tl(corners.top_left)
        .rounded_tr(corners.top_right)
        .rounded_br(corners.bottom_right)
        .rounded_bl(corners.bottom_left)
}

/// The cover at `width` and the art's aspect, cropped to fill, or the placeholder in its
/// place. The picture carries the corners itself, since gpui clips a child to its
/// parent's rectangle, not its radius.
pub fn art(
    shown: Shown,
    name: &str,
    kind: Kind,
    width: f32,
    corners: Corners<Pixels>,
    cx: &App,
) -> Div {
    match shown {
        Shown::Ready(image) => div()
            .flex_none()
            .w(px(width))
            .h(px(grid::cover_height(width)))
            .child(rounded(
                img(image).size_full().object_fit(ObjectFit::Cover),
                corners,
            )),
        Shown::Placeholder(look) => placeholder::render(name, kind, look, width, corners, cx),
    }
}

#[cfg(test)]
mod tests {
    use super::clip;

    #[test]
    fn clip_makes_one_plain_line_of_whole_characters() {
        assert_eq!(clip("  a\nb\r\n\tc\u{1b}[2J  ", 50), "a b c [2J");
        assert_eq!(clip("ééééé", 3), "ééé\u{2026}");
        assert_eq!(clip("👍🏽👍🏽", 1), "👍\u{2026}");
        assert_eq!(clip("abc", 3), "abc");
        assert_eq!(clip("", 5), "");
    }
}
