//! A small geometric mark for each subagent, drawn from its thread ID: the same agent
//! always gets the same spiral, rose, star, knot, spirograph, Maurer rose or
//! superformula shape.

use std::f32::consts::TAU;

use dusk_protocol::thread::fnv1a;
use gpui_kit::*;

const SIZE: f32 = 30.;
const STEPS: usize = 360;

#[derive(Clone, Copy)]
enum Shape {
    /// An Archimedean spiral of `turns` turns.
    Spiral { turns: f32 },
    /// r = cos(k t): a flower of `petals` petals.
    Rose { petals: f32 },
    /// Every `skip`-th of `points` points joined: a star polygon.
    Star { points: usize, skip: usize },
    /// A Lissajous figure.
    Knot { a: f32, b: f32, phase: f32 },
    /// A spirograph: a circle of radius `r` rolling inside (`inside`) or outside a
    /// circle of radius 1, drawing from `d` off its center.
    Spirograph { r: f32, d: f32, inside: bool },
    /// A Maurer rose: the rose r = sin(n t) walked in steps of `degrees`.
    Maurer { n: f32, degrees: f32 },
    /// Gielis' superformula with `m`-fold symmetry.
    Super { m: f32, n1: f32, n23: f32 },
}

fn shape(seed: u64) -> Shape {
    let pick = |shift: u32, count: u64| (seed >> shift) % count;
    match seed % 7 {
        0 => Shape::Spiral {
            turns: 2. + pick(8, 3) as f32,
        },
        1 => Shape::Rose {
            petals: [3., 4., 5., 6., 7.][pick(8, 5) as usize],
        },
        2 => {
            let points = 5 + pick(8, 5) as usize;
            Shape::Star {
                points,
                skip: 2 + pick(16, (points as u64 - 3) / 2) as usize,
            }
        }
        3 => Shape::Knot {
            a: 1. + pick(8, 4) as f32,
            b: 2. + pick(16, 3) as f32,
            phase: pick(24, 8) as f32 * TAU / 16.,
        },
        4 => Shape::Spirograph {
            // Small whole ratios close the curve within the drawn turns.
            r: [1. / 3., 2. / 5., 3. / 7., 1. / 4., 3. / 8.][pick(8, 5) as usize],
            d: 0.4 + pick(16, 5) as f32 * 0.12,
            inside: pick(24, 2) == 0,
        },
        5 => Shape::Maurer {
            n: [2., 3., 4., 5., 6., 7.][pick(8, 6) as usize],
            degrees: [29., 37., 71., 97., 113.][pick(16, 5) as usize],
        },
        _ => Shape::Super {
            m: [3., 4., 5., 6., 7., 8.][pick(8, 6) as usize],
            n1: [0.3, 0.5, 1., 2.][pick(16, 4) as usize],
            n23: [0.3, 1., 1.7][pick(24, 3) as usize],
        },
    }
}

/// The points scaled to fit the unit circle, whatever size the formula gave.
fn fit(points: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
    let largest = points
        .iter()
        .map(|(x, y)| (x * x + y * y).sqrt())
        .filter(|r| r.is_finite())
        .fold(0., f32::max);
    if largest <= f32::EPSILON {
        return points;
    }
    points
        .into_iter()
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .map(|(x, y)| (x / largest, y / largest))
        .collect()
}

/// Points of the shape in a unit circle.
fn points(shape: Shape) -> Vec<(f32, f32)> {
    match shape {
        Shape::Spiral { turns } => (0..=STEPS)
            .map(|step| {
                let t = step as f32 / STEPS as f32;
                let angle = t * turns * TAU;
                (t * angle.cos(), t * angle.sin())
            })
            .collect(),
        Shape::Rose { petals } => {
            // An odd k closes in half a turn; drawing a whole turn covers both cases.
            (0..=STEPS)
                .map(|step| {
                    let angle = step as f32 / STEPS as f32 * TAU;
                    let radius = (petals * angle).cos();
                    (radius * angle.cos(), radius * angle.sin())
                })
                .collect()
        }
        Shape::Star { points, skip } => (0..=points)
            .map(|index| {
                let angle = (index * skip % points) as f32 / points as f32 * TAU - TAU / 4.;
                (angle.cos(), angle.sin())
            })
            .collect(),
        Shape::Knot { a, b, phase } => (0..=STEPS)
            .map(|step| {
                let t = step as f32 / STEPS as f32 * TAU;
                ((a * t + phase).sin(), (b * t).sin())
            })
            .collect(),
        Shape::Spirograph { r, d, inside } => {
            // The curve closes after as many turns as the ratio's denominator.
            let turns = (1. / r).ceil().max(1.) * 3.;
            let steps = STEPS * 3;
            fit((0..=steps)
                .map(|step| {
                    let t = step as f32 / steps as f32 * turns * TAU;
                    let (big, k) = if inside {
                        (1. - r, (1. - r) / r)
                    } else {
                        (1. + r, (1. + r) / r)
                    };
                    let sign = if inside { 1. } else { -1. };
                    let x = big * t.cos() + sign * d * r * (k * t).cos();
                    let y = big * t.sin() - d * r * (k * t).sin();
                    (x, y)
                })
                .collect())
        }
        Shape::Maurer { n, degrees } => (0..=360)
            .map(|step| {
                let angle = (step as f32 * degrees).to_radians();
                let radius = (n * angle).sin();
                (radius * angle.cos(), radius * angle.sin())
            })
            .collect(),
        Shape::Super { m, n1, n23 } => fit((0..=STEPS)
            .map(|step| {
                let angle = step as f32 / STEPS as f32 * TAU;
                let part = |f: f32| (f.abs()).powf(n23);
                let sum = part((m * angle / 4.).cos()) + part((m * angle / 4.).sin());
                let radius = sum.powf(-1. / n1);
                (radius * angle.cos(), radius * angle.sin())
            })
            .collect()),
    }
}

pub fn render(thread_id: &str, color: Hsla) -> impl IntoElement {
    let points = points(shape(fnv1a(thread_id.as_bytes())));
    div().size(px(SIZE)).flex_none().child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let center = bounds.center();
                let radius = bounds.size.width.min(bounds.size.height) / 2. * 0.86;
                let at = |(x, y): (f32, f32)| point(center.x + radius * x, center.y + radius * y);
                let mut path = PathBuilder::stroke(px(1.4));
                let mut iter = points.iter().copied();
                if let Some(first) = iter.next() {
                    path.move_to(at(first));
                }
                for next in iter {
                    path.line_to(at(next));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            },
        )
        .size_full(),
    )
}

#[cfg(test)]
mod tests {
    // Not a glob: gpui exports a `test` macro that would replace the standard one.
    use super::{Shape, fnv1a, points, shape};

    #[test]
    fn every_seed_draws_a_finite_shape() {
        for seed in 0..2000u64 {
            let seed = fnv1a(&seed.to_le_bytes());
            let shape = shape(seed);
            if let Shape::Star { points, skip } = shape {
                assert!(skip >= 2 && skip < points, "{points} {skip}");
            }
            let drawn = points(shape);
            assert!(drawn.len() > 3);
            assert!(
                drawn
                    .iter()
                    .all(|(x, y)| x.abs() <= 1.001 && y.abs() <= 1.001)
            );
        }
    }
}
