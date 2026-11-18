//! The small drawings of a block, painted in code from strokes, so no image file is
//! fetched or shipped: the state of a check and the arrow between the layers of a flow.
//! Colors come from the caller's theme.

use std::f32::consts::{FRAC_PI_2, TAU};

use dusk_protocol::thread::CheckState;
use gpui_kit::*;

/// Side of a state glyph. Its strokes are drawn on a 14 unit square and scaled to this.
pub const SIZE: f32 = 16.;
/// The arrow's box: a 12 px line and a chevron, centred in the 28 px between layers.
pub const ARROW_WIDTH: f32 = 16.;
pub const ARROW_HEIGHT: f32 = 28.;
const RING_STEPS: usize = 28;

/// The colors a glyph is painted in.
#[derive(Clone, Copy)]
pub struct Tones {
    pub muted: Hsla,
    pub accent: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    /// The text color at a low alpha, for the track of a ring.
    pub track: Hsla,
}

/// One state of a check: queued is a ring, running a half ring that stands still, passed
/// a tick, failed a cross, skipped a dash. The word of the state is shown by the caller.
pub fn state(state: CheckState, tones: Tones) -> impl IntoElement {
    div().flex_none().size(px(SIZE)).child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let unit = SIZE / 14.;
                let at = |x: f32, y: f32| {
                    point(
                        bounds.origin.x + px(x * unit),
                        bounds.origin.y + px(y * unit),
                    )
                };
                let stroke =
                    |points: &[(f32, f32)], width: f32, color: Hsla, window: &mut Window| {
                        paint_line(
                            points.iter().map(|&(x, y)| at(x, y)),
                            width * unit,
                            color,
                            window,
                        )
                    };
                match state {
                    CheckState::Passed => stroke(
                        &[(3., 7.4), (5.6, 10.), (11., 4.4)],
                        1.6,
                        tones.success,
                        window,
                    ),
                    CheckState::Failed => {
                        stroke(&[(4., 4.), (10., 10.)], 1.6, tones.danger, window);
                        stroke(&[(10., 4.), (4., 10.)], 1.6, tones.danger, window);
                    }
                    CheckState::Skipped => stroke(&[(4., 7.), (10., 7.)], 1.6, tones.muted, window),
                    CheckState::Queued => {
                        stroke(&arc(0., TAU), 1.5, tones.muted, window);
                    }
                    CheckState::Running => {
                        stroke(&arc(0., TAU), 1.5, tones.track, window);
                        // The right half, from the top down.
                        stroke(&arc(-FRAC_PI_2, FRAC_PI_2), 1.5, tones.accent, window);
                    }
                }
            },
        )
        .size_full(),
    )
}

/// The arrow between two layers of a flow, pointing down.
pub fn arrow(color: Hsla) -> impl IntoElement {
    div()
        .flex_none()
        .w(px(ARROW_WIDTH))
        .h(px(ARROW_HEIGHT))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let at =
                        |x: f32, y: f32| point(bounds.origin.x + px(x), bounds.origin.y + px(y));
                    paint_line([at(8., 6.), at(8., 20.)].into_iter(), 1.5, color, window);
                    paint_line(
                        [at(4., 16.), at(8., 20.), at(12., 16.)].into_iter(),
                        1.5,
                        color,
                        window,
                    );
                },
            )
            .size_full(),
        )
}

/// Points of a ring of radius 4.5 around (7, 7), from angle `from` to `to`.
fn arc(from: f32, to: f32) -> Vec<(f32, f32)> {
    (0..=RING_STEPS)
        .map(|step| {
            let angle = from + (to - from) * step as f32 / RING_STEPS as f32;
            (7. + 4.5 * angle.cos(), 7. + 4.5 * angle.sin())
        })
        .collect()
}

fn paint_line(
    mut points: impl Iterator<Item = Point<Pixels>>,
    width: f32,
    color: Hsla,
    window: &mut Window,
) {
    let Some(first) = points.next() else {
        return;
    };
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(first);
    for next in points {
        path.line_to(next);
    }
    // A path the tessellator refuses is left out: a glyph is not worth an error.
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}
