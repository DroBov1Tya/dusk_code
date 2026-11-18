//! Featured: the official picks as a wide hero, one at a time. The cover fills the left
//! part at the art's aspect and fades into the panel, where the name, what it is, its
//! tags and facts are, with its button always at the panel's foot. A strip of the picks'
//! covers under it jumps to one; arrows step. It does not move by itself.

use std::time::Duration;

use dusk_protocol::plugin::{Listing, Tier};
use gpui_kit::{
    component::{ActiveTheme, Icon, IconName},
    prelude::FluentBuilder,
    *,
};

use super::{Grid, actions, capsule, grid, tier};
use crate::{
    ui::{Type, size},
    workspace::Workspace,
};

const PICKS: usize = 5;
/// The share of the hero's width the cover takes.
const ART_SHARE: f32 = 0.62;
/// A narrower hero puts its text under the cover.
const SIDE_BY_SIDE: f32 = 720.;
/// The width of a cover in the strip.
const THUMB: f32 = 128.;
/// The crossfade between picks: the change is seen, not chased.
const FADE: Duration = Duration::from_millis(280);

/// The first official listings, in the core's order.
pub fn featured<'a>(shown: &[&'a Listing]) -> Vec<&'a Listing> {
    shown
        .iter()
        .copied()
        .filter(|listing| listing.tier == Tier::Official)
        .take(PICKS)
        .collect()
}

pub fn render(ws: &Workspace, picks: &[&Listing], grid: Grid, cx: &mut Context<Workspace>) -> Div {
    let count = picks.len();
    let index = ws.workshop.hero % count;
    let listing = picks[index];
    let source = match listing.source.as_str() {
        "" => "duskcode".to_owned(),
        source => source.to_owned(),
    };
    let mut panel = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_S))
        .min_w_0()
        .child(
            div()
                .meta()
                .text_color(cx.theme().muted_foreground)
                .child(format!("Featured \u{b7} from {source}")),
        )
        .child(div().display().truncate().child(listing.name.clone()))
        .child(
            div()
                .body()
                .text_color(cx.theme().muted_foreground)
                .line_clamp(3)
                .text_ellipsis()
                .child(listing.description.clone()),
        )
        .child(facts(listing, cx));
    if let Some(tags) = tags(listing, cx) {
        panel = panel.child(tags);
    }
    let foot = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_S))
        .h(px(size::CONTROL))
        .child(actions::slot(listing, ws, false, cx));
    let opened = listing.clone();
    let more = div()
        .id("hero-more")
        .meta()
        .cursor_pointer()
        .text_color(cx.theme().muted_foreground)
        .hover(|link| link.underline())
        .child("Details")
        .on_click(cx.listener(move |ws, _, window, cx| ws.open_listing(&opened, window, cx)));
    let foot = foot.child(div().flex_1()).child(more);
    let art = capsule::art(
        ws.workshop.covers.get(listing),
        &listing.name,
        listing.tier,
        art_width(grid.width),
        Corners::default(),
        cx,
    );
    let mut hero = banner(art, grid.width, panel, foot, cx);
    if count > 1 {
        let art_w = art_width(grid.width);
        let top = (grid::cover_height(art_w) - size::CONTROL) / 2.;
        hero = hero
            .child(
                arrow("hero-back", IconName::ChevronLeft, count - 1, count, cx)
                    .top(px(top))
                    .left(px(size::GAP_M)),
            )
            .child(
                arrow("hero-next", IconName::ChevronRight, 1, count, cx)
                    .top(px(top))
                    .left(px(art_w - size::CONTROL - size::GAP_M)),
            );
    }
    let mut column = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_M))
        // Each pick fades in as it comes, so the change is seen without a slide.
        .child(
            hero.with_animation(("hero", index), Animation::new(FADE), |hero, delta| {
                hero.opacity(0.35 + 0.65 * delta)
            }),
        );
    if count > 1 {
        column = column.child(strip(ws, picks, index, cx));
    }
    column
}

pub fn art_width(width: f32) -> f32 {
    if width < SIDE_BY_SIDE {
        width
    } else {
        (width * ART_SHARE).floor()
    }
}

/// A cover with its panel: side by side, the cover fading into the panel; narrow, the
/// panel under it. The pack's page draws its banner with this too.
pub fn banner(art: Div, width: f32, panel: Div, foot: Div, cx: &App) -> Div {
    let theme = cx.theme();
    let (surface, border) = (theme.muted, theme.border);
    let art_w = art_width(width);
    let height = grid::cover_height(art_w);
    let frame = div()
        .relative()
        .w(px(width))
        .rounded_xl()
        .overflow_hidden()
        .border_1()
        .border_color(border)
        .bg(surface);
    if width < SIDE_BY_SIDE {
        return frame.flex().flex_col().child(art).child(
            div()
                .flex()
                .flex_col()
                .gap(px(size::GAP_L))
                .p(px(size::GAP_XL))
                .child(panel)
                .child(foot),
        );
    }
    // On a dark surface the cover's last third fades into the panel. On a light one the
    // dark art would fade through grey, so it ends at a clean edge and the panel keeps
    // its own margin; the text never sits on the art either way.
    let dark = cx.theme().is_dark();
    let fade = (art_w * 0.34).floor();
    let mut frame = frame
        .h(px(height))
        .child(div().absolute().top_0().left_0().child(art));
    if dark {
        frame = frame.child(
            div()
                .absolute()
                .top_0()
                .left(px(art_w - fade))
                .w(px(fade + 1.))
                .h(px(height))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(surface.opacity(0.), 0.),
                    linear_color_stop(surface, 1.),
                )),
        );
    }
    let inset = if dark { 0. } else { size::GAP_XL };
    frame.child(
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(art_w + inset))
            .right_0()
            .flex()
            .flex_col()
            .justify_between()
            .py(px(size::GAP_XL))
            .pr(px(size::GAP_XL))
            .child(panel)
            .child(foot),
    )
}

/// The tier's mark, the kind, the stars and the version on one line.
pub fn facts(listing: &Listing, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(size::GAP_M))
        .child(tier::mark(listing.tier, cx))
        .child(
            div()
                .meta()
                .text_color(muted)
                .child(super::kind_word(listing.kind)),
        )
        .when(listing.stars > 0, |row| {
            row.child(actions::stars(listing.stars, cx))
        })
        .child(
            div()
                .meta()
                .text_color(muted)
                .child(format!("v{}", listing.version)),
        )
}

/// The category and tags as quiet chips; none when it has neither.
pub fn tags(listing: &Listing, cx: &App) -> Option<Div> {
    let words: Vec<String> = std::iter::once(&listing.category)
        .chain(&listing.tags)
        .filter(|word| !word.is_empty())
        .map(|word| crate::ui::widgets::capitalize(word))
        .collect();
    if words.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let (muted, chip) = (theme.muted_foreground, theme.background);
    Some(
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(size::GAP_XS))
            .children(words.into_iter().map(move |word| {
                div()
                    .flex()
                    .items_center()
                    .h(px(size::GAP_XL))
                    .px(px(size::GAP_S))
                    .rounded_md()
                    .bg(chip)
                    .meta()
                    .text_color(muted)
                    .child(word)
            })),
    )
}

/// A round arrow on the cover's edge, on the page's colour so it reads on any art.
fn arrow(
    id: &'static str,
    icon: IconName,
    step: usize,
    count: usize,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    round(id, icon, cx).on_click(cx.listener(move |ws, _, _, cx| {
        ws.workshop.hero = (ws.workshop.hero % count + step) % count;
        cx.notify();
    }))
}

/// A round button of a control's size over a picture; the gallery's arrows are these too.
pub fn round(id: impl Into<ElementId>, icon: IconName, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (back, hover, foreground) = (theme.background, theme.list_hover, theme.foreground);
    div()
        .id(id)
        .absolute()
        .size(px(size::CONTROL))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(back.opacity(0.85))
        .hover(move |arrow| arrow.bg(hover))
        .child(Icon::new(icon).size(px(size::ICON)).text_color(foreground))
}

/// The picks' covers in a row; the one in view is ringed, the others quiet.
fn strip(ws: &Workspace, picks: &[&Listing], index: usize, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (ring, border) = (theme.primary, theme.border);
    div()
        .flex()
        .flex_row()
        .gap(px(size::GAP_S))
        .children(picks.iter().enumerate().map(|(each, listing)| {
            let chosen = each == index;
            div()
                .id(("hero-thumb", each))
                .flex_none()
                .rounded_md()
                .overflow_hidden()
                .border_2()
                .border_color(if chosen { ring } else { border })
                .cursor_pointer()
                .opacity(if chosen { 1. } else { 0.6 })
                .hover(|thumb| thumb.opacity(1.))
                .child(capsule::art(
                    ws.workshop.covers.get(listing),
                    &listing.name,
                    listing.tier,
                    THUMB,
                    Corners::default(),
                    cx,
                ))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.workshop.hero = each;
                    cx.notify();
                }))
        }))
}
