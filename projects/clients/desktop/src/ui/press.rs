//! How a row, a card or a tile answers a press: its fill dips and its top edge sinks by
//! one pixel, at once, and both go back on release.
//!
//! gpui cannot scale an element (only an svg takes a transform), and a spring would wrap
//! each element in one of its own, so the answer is a style, not an animation: it costs
//! nothing while idle and there is no motion to reduce. The kit's buttons draw their own
//! pressed fill from the theme.

use gpui_kit::{component::ActiveTheme, *};

/// The edge a pressed surface shows inside its top: the shadow of the rim it sank under,
/// in the theme's ink.
pub fn dent(cx: &App) -> Vec<BoxShadow> {
    let theme = cx.theme();
    let ink = if theme.mode.is_dark() {
        Hsla {
            l: 0.,
            ..theme.background
        }
    } else {
        theme.foreground
    };
    let dark = theme.mode.is_dark();
    let inside = |strength: f32, y: f32, blur: f32, spread: f32| BoxShadow {
        color: ink.opacity(strength),
        offset: point(px(0.), px(y)),
        blur_radius: px(blur),
        spread_radius: px(spread),
        inset: true,
    };
    vec![
        // A veil over the whole surface: an inner shadow spread past its middle, so the
        // dip is the same whatever the fill under it.
        inside(if dark { 0.22 } else { 0.05 }, 0., 0., VEIL),
        inside(if dark { 0.5 } else { 0.16 }, 1., 1., 0.),
    ]
}

/// A spread no surface here is half as wide as.
const VEIL: f32 = 4000.;

pub trait Press: StatefulInteractiveElement + Sized {
    /// While pressed the element takes `fill` and the dent. An element takes one pressed
    /// style, so this stands in the place of `active`.
    fn press(self, fill: impl Into<Fill>, cx: &App) -> Self {
        let (fill, dent) = (fill.into(), dent(cx));
        self.active(move |style| style.bg(fill).shadow(dent))
    }
}

impl<E: StatefulInteractiveElement + Sized> Press for E {}
