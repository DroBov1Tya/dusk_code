//! How high a surface stands over the page. Three levels, and this is the only place that
//! knows what each one looks like, so a card is the same card on every page.
//!
//! - `Base`: the page itself. No edge.
//! - `Raised`: a card, the message box, a tile. A hairline; on a light theme a soft shadow
//!   under it, on a dark one a lit top edge, as a shadow does not show on a dark page.
//! - `Floating`: a menu, a popover, a window over the page. The same edge and a deep shadow.

use gpui_kit::{component::ActiveTheme, *};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Base,
    Raised,
    Floating,
}

/// The fill of a level.
pub fn surface(level: Level, cx: &App) -> Hsla {
    let theme = cx.theme();
    match level {
        Level::Base => theme.background,
        Level::Raised => theme.muted,
        Level::Floating => theme.popover,
    }
}

/// The shadows of a level: what lies under the surface, and on a dark theme the light its
/// top edge catches.
pub fn shadows(level: Level, cx: &App) -> Vec<BoxShadow> {
    let theme = cx.theme();
    let dark = theme.mode.is_dark();
    // A shadow is the ink of the theme, never a color of its own: the text color on a
    // light page, the page's own hue at no lightness on a dark one.
    let ink = if dark {
        Hsla {
            l: 0.,
            ..theme.background
        }
    } else {
        theme.foreground
    };
    let cast = |y: f32, blur: f32, strength: f32| BoxShadow {
        color: ink.opacity(strength),
        offset: point(px(0.), px(y)),
        blur_radius: px(blur),
        spread_radius: px(0.),
        inset: false,
    };
    let lit = BoxShadow {
        color: theme.foreground.opacity(0.07),
        offset: point(px(0.), px(1.)),
        blur_radius: px(0.),
        spread_radius: px(0.),
        inset: true,
    };
    match (level, dark) {
        (Level::Base, _) => Vec::new(),
        (Level::Raised, false) => vec![cast(1., 2., 0.05), cast(6., 16., 0.06)],
        (Level::Raised, true) => vec![lit],
        (Level::Floating, false) => vec![cast(2., 6., 0.08), cast(12., 32., 0.16)],
        (Level::Floating, true) => vec![lit, cast(12., 32., 0.55)],
    }
}

pub trait Elevation: Styled + Sized {
    /// Stands the element at `level`: its fill, its hairline and its shadows. A place that
    /// needs another fill or edge color sets it after this call.
    fn elevation(self, level: Level, cx: &App) -> Self {
        let fill = surface(level, cx);
        if level == Level::Base {
            return self.bg(fill);
        }
        self.bg(fill)
            .border_1()
            .border_color(cx.theme().border)
            .shadow(shadows(level, cx))
    }
}

impl<E: Styled + Sized> Elevation for E {}
