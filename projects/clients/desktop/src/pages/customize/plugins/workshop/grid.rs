//! The Workshop's grid: the page's width, how many cards fit a row and how wide each is,
//! so the hero, every shelf and the results line up on the same columns. The width is
//! the page's own, measured where it is drawn; until the first measure it is taken
//! from the middle column. Covers keep the art's 40:21 at every size.

use std::{cell::Cell, rc::Rc};

use gpui_kit::*;

use crate::{ui::size, workspace::Workspace};

/// The covers' width over their height, as the store's art is drawn (1200x630).
pub const ASPECT: f32 = 40. / 21.;
/// Wider than this the page stops growing.
const MAX_WIDTH: f32 = 1184.;
/// A card narrower than this would cut its name and button.
const CARD_MIN: f32 = 232.;
/// Five never fit a page that stops at 1184.
const COLUMNS_MAX: usize = 4;
/// The space between cards and between a shelf's parts.
pub const GAP: f32 = size::GAP_L;

#[derive(Clone, Copy)]
pub struct Grid {
    pub width: f32,
    pub columns: usize,
    pub capsule: f32,
}

/// The page's width as it was last laid out, shared with the canvas that measures it.
#[derive(Default)]
pub struct Measured(Rc<Cell<Option<f32>>>);

impl Grid {
    pub fn of(ws: &Workspace, window: &Window) -> Self {
        // Before the first measure: the middle column less the page's own margins.
        let room = ws
            .workshop
            .measured
            .0
            .get()
            .unwrap_or_else(|| ws.chat_width(window) - 2. * size::GAP_XXL);
        Self::at(room)
    }

    fn at(room: f32) -> Self {
        let width = room.clamp(CARD_MIN, MAX_WIDTH).floor();
        let columns = (((width + GAP) / (CARD_MIN + GAP)) as usize).clamp(1, COLUMNS_MAX);
        let capsule = ((width - GAP * (columns - 1) as f32) / columns as f32).floor();
        Self {
            width,
            columns,
            capsule,
        }
    }
}

/// Laid under the page at its full width: notes the width and, when it changed, draws
/// the page again on it.
pub fn measure(measured: &Measured) -> impl IntoElement {
    let measured = Rc::clone(&measured.0);
    canvas(
        move |bounds, window, _| {
            let width = bounds.size.width.as_f32().floor();
            if measured.get() != Some(width) {
                measured.set(Some(width));
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A cover's height at `width`.
pub fn cover_height(width: f32) -> f32 {
    (width / ASPECT).round()
}
