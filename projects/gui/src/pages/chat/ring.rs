//! The context meter under the composer: a ring in four equal quarters, filled as the
//! context fills and colored by how full it is, so its state reads at a glance.

use std::f32::consts::{FRAC_PI_2, TAU};

use gpui_kit::*;

const SIZE: f32 = 18.;
const STEPS: usize = 24;
/// The gap between quarters, in radians.
const GAP: f32 = 0.22;

pub fn render(share: f32, fill: Hsla, track: Hsla) -> impl IntoElement {
    let share = share.clamp(0., 1.);
    div().size(px(SIZE)).flex_none().child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let center = bounds.center();
                let radius = bounds.size.width.min(bounds.size.height) / 2. - px(2.);
                let arc = |from: f32, to: f32, color: Hsla, window: &mut Window| {
                    if to <= from {
                        return;
                    }
                    let mut path = PathBuilder::stroke(px(2.5));
                    for step in 0..=STEPS {
                        let angle = from + (to - from) * step as f32 / STEPS as f32 - FRAC_PI_2;
                        let at = point(
                            center.x + radius * angle.cos(),
                            center.y + radius * angle.sin(),
                        );
                        if step == 0 {
                            path.move_to(at);
                        } else {
                            path.line_to(at);
                        }
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                };
                let quarter = TAU / 4.;
                for part in 0..4 {
                    let from = part as f32 * quarter + GAP / 2.;
                    let to = from + quarter - GAP;
                    arc(from, to, track, window);
                    // How much of this quarter the context covers.
                    let covered = (share * 4. - part as f32).clamp(0., 1.);
                    arc(from, from + (to - from) * covered, fill, window);
                }
            },
        )
        .size_full(),
    )
}
