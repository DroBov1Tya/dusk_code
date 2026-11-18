//! What stands where a cover is not: drawn here from the theme, no file and no network,
//! and quieter than any cover. The kind's glyph for a listing without one; an empty box
//! while its cover comes; the glyph and "Offline" for one that could not be fetched while
//! the store could not be read.

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
    if look == Look::Coming {
        return frame;
    }
    let frame = frame.child(
        Icon::default()
            .path(glyph(kind))
            .size(px(GLYPH))
            .text_color(muted.opacity(0.6)),
    );
    if look == Look::None {
        return frame;
    }
    frame.child(
        div()
            .absolute()
            .right(px(size::GAP_S))
            .bottom(px(size::GAP_S))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(size::GAP_XS))
            .meta()
            .text_color(muted)
            .child(
                Icon::default()
                    .path(icons::CLOUD_OFF)
                    .size(px(OFFLINE_ICON))
                    .text_color(muted),
            )
            .child("Offline"),
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
