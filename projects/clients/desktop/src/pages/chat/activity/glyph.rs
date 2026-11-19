//! The marks, each drawn in a square of `SIZE` from a phase of its loop. At phase zero,
//! where a loop stands under reduced motion, each shows a whole, quiet figure.

use std::{
    f32::consts::{FRAC_PI_2, PI, TAU},
    time::Duration,
};

use gpui_kit::*;

use crate::ui::looping::Loop;

pub const SIZE: f32 = 16.;
const MIDDLE: f32 = SIZE / 2.;

/// A square that `paint` draws at each tick with the phase of a loop of `period`; it is
/// given the square's corner.
fn drawn(
    width: f32,
    period: Duration,
    paint: impl Fn(Point<Pixels>, f32, &mut Window) + 'static,
) -> impl IntoElement {
    div()
        .flex_none()
        .w(px(width))
        .h(px(SIZE))
        .looping(period, move |holder, phase| {
            holder.child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| paint(bounds.origin, phase, window),
                )
                .size_full(),
            )
        })
}

/// A square drawn once: a mark that stands still asks for no frame.
fn fixed(paint: impl Fn(Point<Pixels>, &mut Window) + 'static) -> impl IntoElement {
    div().flex_none().size(px(SIZE)).child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| paint(bounds.origin, window),
        )
        .size_full(),
    )
}

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

fn at(origin: Point<Pixels>, x: f32, y: f32) -> Point<Pixels> {
    point(origin.x + px(x), origin.y + px(y))
}

fn disc(origin: Point<Pixels>, x: f32, y: f32, radius: f32, color: Hsla, window: &mut Window) {
    let bounds = Bounds::new(
        at(origin, x - radius, y - radius),
        size(px(radius * 2.), px(radius * 2.)),
    );
    window.paint_quad(fill(bounds, color).corner_radii(px(radius)));
}

/// A line with round ends, `high` thick, from `x` to `x + wide` around the height `y`.
fn bar(
    origin: Point<Pixels>,
    (x, y): (f32, f32),
    (wide, high): (f32, f32),
    color: Hsla,
    window: &mut Window,
) {
    if wide <= 0. {
        return;
    }
    let bounds = Bounds::new(at(origin, x, y - high / 2.), size(px(wide), px(high)));
    window.paint_quad(fill(bounds, color).corner_radii(px(high / 2.)));
}

fn stroke(
    points: impl IntoIterator<Item = Point<Pixels>>,
    width: f32,
    color: Hsla,
    window: &mut Window,
) {
    let mut points = points.into_iter();
    let Some(first) = points.next() else {
        return;
    };
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(first);
    for next in points {
        path.line_to(next);
    }
    // A path the tessellator refuses is left out: a mark is not worth an error.
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// The shapes the spark passes through, each as how far its rays reach, how thick they
/// are at the middle and at the tip, in shares of half the square: a pointed star, petals,
/// a stout star, a dot, a thin asterisk.
const SHAPES: [[f32; 3]; 5] = [
    [0.84, 0.30, 0.05],
    [0.90, 0.09, 0.30],
    [0.66, 0.22, 0.22],
    [0.16, 0.36, 0.36],
    [0.94, 0.11, 0.11],
];
/// The share of each shape's time it stands before easing into the next.
const HOLD: f32 = 0.42;

/// Where the spark is at `phase`: its shape and how far it has turned. It turns a sixth
/// between shapes, which six rays hide once it stands, so the loop has no seam.
fn spark_at(phase: f32) -> ([f32; 3], f32) {
    let through = phase.clamp(0., 1.) * SHAPES.len() as f32;
    let index = (through as usize).min(SHAPES.len() - 1);
    let moved = ease_in_out((through - index as f32 - HOLD) / (1. - HOLD));
    let (from, to) = (SHAPES[index], SHAPES[(index + 1) % SHAPES.len()]);
    let shape = std::array::from_fn(|part| from[part] + (to[part] - from[part]) * moved);
    (shape, (index as f32 + moved) * TAU / 6.)
}

/// Thinking: six rays that ease from shape to shape.
pub fn spark(color: Hsla) -> impl IntoElement {
    drawn(
        SIZE,
        Duration::from_millis(2600),
        move |origin, phase, window| {
            let ([reach, middle, tip], turned) = spark_at(phase);
            let half = MIDDLE - 1.;
            let (reach, middle, tip) = (reach * half, middle * half, tip * half);
            let long = (reach - tip / 2.).max(0.);
            for ray in 0..6 {
                let angle = turned - FRAC_PI_2 + ray as f32 * TAU / 6.;
                let (sin, cos) = angle.sin_cos();
                // Along the ray and across it, from the middle of the square.
                let place = |along: f32, across: f32| {
                    at(
                        origin,
                        MIDDLE + along * cos - across * sin,
                        MIDDLE + along * sin + across * cos,
                    )
                };
                let mut path = PathBuilder::fill();
                path.move_to(place(0., -middle / 2.));
                // The tip is round: half a turn in a few steps.
                for step in 0..=6 {
                    let around = -FRAC_PI_2 + PI * step as f32 / 6.;
                    path.line_to(place(
                        long + tip / 2. * around.cos(),
                        tip / 2. * around.sin(),
                    ));
                }
                path.line_to(place(0., middle / 2.));
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
            disc(origin, MIDDLE, MIDDLE, middle / 2., color, window);
        },
    )
}

/// Writing: a dot that does not move.
pub fn still_dot(color: Hsla) -> impl IntoElement {
    fixed(move |origin, window| disc(origin, MIDDLE, MIDDLE, 2.5, color, window))
}

/// A running step: a dot whose ring goes out from it and fades.
pub fn live(color: Hsla) -> impl IntoElement {
    drawn(
        SIZE,
        Duration::from_millis(1600),
        move |origin, phase, window| {
            let out = ease_out(phase / 0.75);
            if phase > 0. && out < 1. {
                let radius = 2.5 + 4.5 * out;
                let ring = (0..=24).map(|step| {
                    let angle = step as f32 * TAU / 24.;
                    at(
                        origin,
                        MIDDLE + radius * angle.cos(),
                        MIDDLE + radius * angle.sin(),
                    )
                });
                stroke(ring, 1.2, color.opacity(0.55 * (1. - out)), window);
            }
            disc(origin, MIDDLE, MIDDLE, 2.5, color, window);
        },
    )
}

/// Planning: three lines written one after another, held, then let go.
pub fn lines(accent: Hsla, line: Hsla) -> impl IntoElement {
    const CYCLE: f32 = 2.2;
    drawn(
        SIZE,
        Duration::from_millis(2200),
        move |origin, phase, window| {
            // The loop starts with the lines written, so they stand when nothing moves.
            let time = (phase * CYCLE + 1.2) % CYCLE;
            let left = 1. - ((time - 1.7) / 0.4).clamp(0., 1.);
            for row in 0..3 {
                let written = ease_out((time - row as f32 * 0.3) / 0.35);
                let y = 4. + row as f32 * 4.;
                let bullet = accent.opacity(left * (written * 4.).min(1.));
                disc(origin, 3., y, 1., bullet, window);
                let wide = 8. * written * if row == 2 { 0.7 } else { 1. };
                bar(
                    origin,
                    (6., y),
                    (wide, 1.5),
                    line.opacity(left * 0.9),
                    window,
                );
            }
        },
    )
}

/// Agents: a dot for each of the burst, four at most; those at work pass a wave along,
/// those that are done are dim.
pub fn dots(working: u32, started: u32, accent: Hsla, done: Hsla) -> impl IntoElement {
    const DOT: f32 = 3.5;
    const GAP: f32 = 2.;
    let shown = started.max(working).clamp(1, 4);
    let lit = working.min(shown);
    let width = super::SLOT;
    drawn(
        width,
        Duration::from_millis(1600),
        move |origin, phase, window| {
            let left = width / 2. - (shown as f32 * DOT + (shown - 1) as f32 * GAP) / 2.;
            for dot in 0..shown {
                let x = left + DOT / 2. + dot as f32 * (DOT + GAP);
                let color = if dot < lit {
                    let wave = (0.5 + 0.5 * ((phase - dot as f32 * 0.16) * TAU).cos()).powi(2);
                    accent.opacity(0.35 + 0.65 * wave)
                } else {
                    done.opacity(0.3)
                };
                disc(origin, x, MIDDLE, DOT / 2., color, window);
            }
        },
    )
}

/// Compacting: three lines drawn together and let apart.
pub fn squeeze(muted: Hsla, text: Hsla) -> impl IntoElement {
    use gpui_kit::component::Colorize;
    drawn(
        SIZE,
        Duration::from_millis(2800),
        move |origin, phase, window| {
            let drawn_in = if phase < 0.4 {
                ease_in_out(phase / 0.4)
            } else if phase < 0.55 {
                1.
            } else {
                1. - ease_in_out((phase - 0.55) / 0.35)
            };
            let color = muted.mix_oklab(text, 0.25 + 0.3 * drawn_in);
            for row in [-1_f32, 0., 1.] {
                let y = MIDDLE + row * (4. - 2.2 * drawn_in);
                let wide = 12. - 5. * drawn_in * row.abs() - 2. * drawn_in;
                let color = color.opacity(1. - 0.35 * drawn_in * row.abs());
                bar(origin, (MIDDLE - wide / 2., y), (wide, 1.5), color, window);
            }
        },
    )
}

/// A steady ring: whole while the turn waits for the user, dotted while the provider
/// holds it. It does not move.
pub fn ring(color: Hsla, dotted: bool) -> impl IntoElement {
    fixed(move |origin, window| {
        const RADIUS: f32 = 3.5;
        let place = |angle: f32| {
            at(
                origin,
                MIDDLE + RADIUS * angle.cos(),
                MIDDLE + RADIUS * angle.sin(),
            )
        };
        if dotted {
            for dot in 0..6 {
                let angle = dot as f32 * TAU / 6.;
                let spot = (RADIUS * angle.cos(), RADIUS * angle.sin());
                disc(origin, MIDDLE + spot.0, MIDDLE + spot.1, 1., color, window);
            }
        } else {
            stroke(
                (0..=28).map(|step| place(step as f32 * TAU / 28.)),
                2.,
                color,
                window,
            );
        }
    })
}

/// A finished step.
pub fn check(color: Hsla) -> impl IntoElement {
    fixed(move |origin, window| {
        let points = [(4., 8.4), (6.8, 11.), (12., 5.4)];
        stroke(points.map(|(x, y)| at(origin, x, y)), 1.4, color, window);
    })
}

/// A failed step.
pub fn cross(color: Hsla) -> impl IntoElement {
    fixed(move |origin, window| {
        for line in [[(4.5, 4.5), (11.5, 11.5)], [(11.5, 4.5), (4.5, 11.5)]] {
            stroke(line.map(|(x, y)| at(origin, x, y)), 1.4, color, window);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{SHAPES, spark_at};

    #[test]
    fn the_spark_stands_on_a_shape_and_its_loop_has_no_seam() {
        // At the start, where reduced motion holds it, it is the whole star.
        assert_eq!(spark_at(0.), (SHAPES[0], 0.));
        // Each shape stands for a while before it moves on.
        let (shape, _) = spark_at(1.2 / 5.);
        assert_eq!(shape, SHAPES[1]);
        // The end of the loop is the first shape again, turned by sixths of a turn,
        // which six rays do not show.
        let (shape, turned) = spark_at(0.999_99);
        for part in 0..3 {
            assert!((shape[part] - SHAPES[0][part]).abs() < 1e-3);
        }
        let sixths = turned / (std::f32::consts::TAU / 6.);
        assert!((sixths - sixths.round()).abs() < 1e-3);
    }
}
