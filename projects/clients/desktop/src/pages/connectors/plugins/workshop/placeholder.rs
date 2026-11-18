//! What stands where a cover is not: drawn here from the theme, no file and no network,
//! and quieter than any cover. The kind's glyph for a listing without one; a quieter
//! glyph marked "Loading" while its cover comes; the glyph and "Offline" for one that
//! could not be fetched while the store could not be read.

use dusk_protocol::plugin::Kind;
use gpui_kit::{
    component::{ActiveTheme, Icon},
    *,
};

use super::capsule;
use crate::ui::{Type, icons, size};

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
pub fn render(kind: Kind, look: Look, width: f32, corners: Corners<Pixels>, cx: &App) -> Div {
    let theme = cx.theme();
    let (raised, hairline, muted) = (theme.secondary, theme.border, theme.muted_foreground);
    let frame = capsule::rounded(div(), corners)
        .relative()
        .flex_none()
        .w(px(width))
        .h(px(super::grid::cover_height(width)))
        .flex()
        .items_center()
        .justify_center()
        .bg(raised)
        .border_1()
        .border_color(hairline);
    // The glyph is quieter while the cover comes: still, since nothing here answers a click.
    let strength = if look == Look::Coming { 0.3 } else { 0.6 };
    let frame = frame.child(
        Icon::default()
            .path(glyph(kind))
            .size(px(GLYPH))
            .text_color(muted.opacity(strength)),
    );
    // A word in the corner tells the three states apart without colour.
    let corner = match look {
        Look::None => return frame,
        Look::Coming => div().child("Loading"),
        Look::Offline => div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(size::GAP_XS))
            .child(
                Icon::default()
                    .path(icons::CLOUD_OFF)
                    .size(px(OFFLINE_ICON))
                    .text_color(muted),
            )
            .child("Offline"),
    };
    frame.child(
        corner
            .absolute()
            .right(px(size::GAP_S))
            .bottom(px(size::GAP_S))
            .meta()
            .text_color(muted),
    )
}

fn glyph(kind: Kind) -> &'static str {
    match kind {
        Kind::Plugin => icons::KIND_PLUGIN,
        Kind::Pack => icons::KIND_PACK,
        Kind::Skill => icons::KIND_SKILL,
        Kind::Agent => icons::KIND_AGENT,
    }
}
