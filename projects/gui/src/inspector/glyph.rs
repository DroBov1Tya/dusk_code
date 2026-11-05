//! A small geometric mark for each subagent, drawn from its thread ID: the same agent
//! always gets the same spiral, rose, star or knot.

use std::f32::consts::TAU;

use dusk_protocol::thread::fnv1a;
use gpui_kit::*;

const SIZE: f32 = 30.;
const STEPS: usize = 240;

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
}

fn shape(seed: u64) -> Shape {
    let pick = |shift: u32, count: u64| (seed >> shift) % count;
    match seed % 4 {
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
        _ => Shape::Knot {
            a: 1. + pick(8, 4) as f32,
            b: 2. + pick(16, 3) as f32,
            phase: pick(24, 8) as f32 * TAU / 16.,
        },
    }
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
            assert!(drawn.iter().all(|(x, y)| x.abs() <= 1.001 && y.abs() <= 1.001));
        }
    }
}
