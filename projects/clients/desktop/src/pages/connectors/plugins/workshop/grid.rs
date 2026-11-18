//! The Workshop's grid: the page's width, how many capsules fit a row and how wide each
//! is, from the middle column's width, so every shelf, the hero and the results line up
//! on the same columns. Covers keep the art's 1200x630 at every size.

use gpui_kit::*;

use crate::{ui::size, workspace::Workspace};

/// The covers' width over their height, as the store's art is drawn.
pub const ASPECT: f32 = 1200. / 630.;
/// Wider than this the page stops growing and stays centered on its columns.
const MAX_WIDTH: f32 = 1184.;
/// A capsule narrower than this would cut its name and button.
const CAPSULE_MIN: f32 = 232.;
const COLUMNS_MAX: usize = 5;
/// The space between capsules and between a shelf's parts.
pub const GAP: f32 = size::GAP_L;

#[derive(Clone, Copy)]
pub struct Grid {
    pub width: f32,
    pub columns: usize,
    pub capsule: f32,
}

impl Grid {
    pub fn of(ws: &Workspace, window: &Window) -> Self {
        // The page's own margins, as the connectors page draws them.
        let room = ws.chat_width(window) - 2. * size::GAP_XXL;
        let width = room.clamp(CAPSULE_MIN, MAX_WIDTH).floor();
        let columns = (((width + GAP) / (CAPSULE_MIN + GAP)) as usize).clamp(1, COLUMNS_MAX);
        let capsule = ((width - GAP * (columns - 1) as f32) / columns as f32).floor();
        Self {
            width,
            columns,
            capsule,
        }
    }
}

/// A cover's height at `width`.
pub fn cover_height(width: f32) -> f32 {
    (width / ASPECT).round()
}
