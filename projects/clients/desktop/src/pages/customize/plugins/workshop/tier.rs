//! The mark of a tier: its icon, colour and word together, so Official is found at a
//! glance and no tier is told by colour alone. The browser draws the same shapes.

use dusk_protocol::plugin::Tier;
use gpui_kit::{
    component::{ActiveTheme, Icon},
    *,
};

use crate::ui::{Type, icons, size};

pub const ALL: [Tier; 5] = [
    Tier::Official,
    Tier::Verified,
    Tier::Community,
    Tier::Unverified,
    Tier::Local,
];

pub fn word(tier: Tier) -> &'static str {
    match tier {
        Tier::Official => "Official",
        Tier::Verified => "Verified",
        Tier::Community => "Community",
        Tier::Unverified => "Unverified",
        Tier::Local => "Local",
    }
}

/// Where a tier's listings come from, in one line.
pub fn about(tier: Tier) -> &'static str {
    match tier {
        Tier::Official => "Made by duskcode and signed by the key built into it.",
        Tier::Verified => "Checked and signed for duskcode; written by somebody else.",
        Tier::Community => "From mirrors you added; no key of duskcode vouches for them.",
        Tier::Unverified => "From a source you added without a key; nobody vouches for them.",
        Tier::Local => "Found on this machine; no source installed them.",
    }
}

/// The accent for Official, the info blue for Verified, a quiet warning for Unverified,
/// quiet for the rest.
pub fn color(tier: Tier, cx: &App) -> Hsla {
    let theme = cx.theme();
    match tier {
        Tier::Official => theme.primary,
        Tier::Verified => theme.info,
        Tier::Community => theme.muted_foreground,
        Tier::Unverified => theme.warning.opacity(0.85),
        Tier::Local => theme.foreground.opacity(0.72),
    }
}

pub fn icon(tier: Tier, size: f32, cx: &App) -> Icon {
    let path = match tier {
        Tier::Official => icons::TIER_OFFICIAL,
        Tier::Verified => icons::TIER_VERIFIED,
        Tier::Community | Tier::Unverified => icons::TIER_COMMUNITY,
        Tier::Local => icons::TIER_LOCAL,
    };
    Icon::default()
        .path(path)
        .size(px(size))
        .text_color(color(tier, cx))
}

/// The icon and the word, tinted: the mark on a card.
pub fn mark(tier: Tier, cx: &App) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_XS))
        .text_color(color(tier, cx))
        .child(icon(tier, size::ICON, cx))
        .child(
            div()
                .meta()
                .font_weight(FontWeight::MEDIUM)
                .child(word(tier)),
        )
}
