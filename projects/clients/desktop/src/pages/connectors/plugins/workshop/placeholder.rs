//! What stands where a cover is not: a gradient picked from the plugin's name (the
//! colours come from `dusk_protocol::plugin::cover_art`, so the browser draws the same
//! ones), no file and no network. The name's first letter large and faint with the kind's
//! glyph for a listing without a cover; the same gradient hushed, with a quiet glyph and
//! "Loading", while its cover comes; hushed, with the glyph and "Offline", for one that
//! could not be fetched while the store could not be read.

use dusk_protocol::plugin::{CoverArt, Hsl, Kind, cover_art};
use gpui_kit::{
    component::{ActiveTheme, Icon},
    *,
};

use super::capsule;
use crate::ui::{Type, avatar, icons, size};

const GLYPH: f32 = 40.;
const OFFLINE_ICON: f32 = 14.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// No cover, or one that failed its checks (the reason is in the log).
    None,
    /// Asked for and not here yet.
    Coming,
    /// Not fetched, and the store could not be read either.
    Offline,
}

/// The cover's box at `width`, rounded by `corners` as the cover would be.
pub fn render(
    name: &str,
    kind: Kind,
    look: Look,
    width: f32,
    corners: Corners<Pixels>,
    cx: &App,
) -> Div {
    let dark = cx.theme().is_dark();
    let art = cover_art(name, dark);
    if look == Look::None {
        return made(name, kind, art, width, corners, cx);
    }
    let art = art.quiet(dark);
    let ink = color(art.ink);
    let frame = frame(art, width, corners, cx)
        .flex()
        .items_center()
        .justify_center();
    // The glyph is quieter while the cover comes: still, since nothing here answers a click.
    let strength = if look == Look::Coming { 0.45 } else { 0.75 };
    let frame = frame.child(
        Icon::default()
            .path(glyph(kind))
            .size(px(GLYPH))
            .text_color(ink.opacity(strength)),
    );
    // A word in the corner tells the three states apart without colour.
    let corner = match look {
        Look::None | Look::Coming => div().child("Loading"),
        Look::Offline => div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(size::GAP_XS))
            .child(
                Icon::default()
                    .path(icons::CLOUD_OFF)
                    .size(px(OFFLINE_ICON))
                    .text_color(ink),
            )
            .child("Offline"),
    };
    frame.child(
        corner
            .absolute()
            .right(px(size::GAP_S))
            .bottom(px(size::GAP_S))
            .meta()
            .text_color(ink),
    )
}

/// A cover made from the name for a listing that brings none: the name's gradient, its
/// first letter large and faint, the kind's glyph in the corner.
fn made(
    name: &str,
    kind: Kind,
    art: CoverArt,
    width: f32,
    corners: Corners<Pixels>,
    cx: &App,
) -> Div {
    let ink = color(art.ink);
    let letter = (super::grid::cover_height(width) * LETTER).round();
    frame(art, width, corners, cx)
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .left(px(size::GAP_L))
                .bottom(px(size::GAP_S))
                .text_size(px(letter))
                .line_height(px(letter))
                .font_weight(FontWeight::BOLD)
                .text_color(ink.opacity(0.2))
                .child(avatar::letter(name)),
        )
        .child(
            Icon::default()
                .path(glyph(kind))
                .absolute()
                .right(px(size::GAP_M))
                .top(px(size::GAP_M))
                .size(px(size::ICON_LARGE))
                .text_color(ink),
        )
}

/// The box every look shares: the cover's size and corners, filled with `art`.
fn frame(art: CoverArt, width: f32, corners: Corners<Pixels>, cx: &App) -> Div {
    capsule::rounded(div(), corners)
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(super::grid::cover_height(width)))
        .bg(linear_gradient(
            art.angle,
            linear_color_stop(color(art.from), 0.),
            linear_color_stop(color(art.to), 1.),
        ))
        .border_1()
        .border_color(cx.theme().border)
}

fn color(c: Hsl) -> Hsla {
    hsla(c.h / 360., c.s, c.l, 1.)
}

/// The letter's height as a share of the cover's.
const LETTER: f32 = 0.6;

fn glyph(kind: Kind) -> &'static str {
    match kind {
        Kind::Plugin => icons::KIND_PLUGIN,
        Kind::Pack => icons::KIND_PACK,
        Kind::Skill => icons::KIND_SKILL,
        Kind::Agent => icons::KIND_AGENT,
    }
}
