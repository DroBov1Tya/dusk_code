//! The arbiter as a sphere of dots: points spread evenly on a sphere turn slowly, the
//! near ones larger and brighter. When the arbiter answers, the sphere stirs like a
//! hive: the dots swell outward, shiver and settle over a second or two.

use std::{
    f32::consts::TAU,
    time::{Duration, Instant},
};

use gpui_kit::*;

const SIZE: f32 = 200.;
const DOTS: usize = 140;
/// The golden angle spreads the points evenly over the sphere.
const GOLDEN: f32 = 2.399_963;
/// How long the sphere stays stirred after an answer.
const SETTLE: f32 = 1.8;

pub fn render(
    id: impl Into<ElementId>,
    last: Option<Instant>,
    calm: Hsla,
    stirred: Hsla,
) -> impl IntoElement {
    div().relative().size(px(SIZE)).flex_none().with_animation(
        id,
        Animation::new(Duration::from_millis(12_000)).repeat(),
        move |sphere, phase| {
            // How stirred the sphere is now: 1 at an answer, fading to 0.
            let stir = last
                .map(|at| (1. - at.elapsed().as_secs_f32() / SETTLE).max(0.))
                .unwrap_or(0.);
            sphere.children(dots(phase, stir * stir, calm, stirred))
        },
    )
}

fn dots(phase: f32, stir: f32, calm: Hsla, stirred: Hsla) -> Vec<Div> {
    let center = SIZE / 2.;
    let turn = phase * TAU;
    let mut points: Vec<(f32, f32, f32)> = (0..DOTS)
        .map(|index| {
            // A Fibonacci sphere: even latitude steps, golden-angle longitude steps.
            let y = 1. - 2. * (index as f32 + 0.5) / DOTS as f32;
            let ring = (1. - y * y).sqrt();
            let longitude = index as f32 * GOLDEN + turn;
            let (x, z) = (ring * longitude.cos(), ring * longitude.sin());
            // Stirred, each dot shivers at its own pace and the whole swells.
            let shiver = (index as f32 * 12.9898 + turn * 40.).sin() * 0.06 * stir;
            let swell = 1. + 0.18 * stir + shiver;
            (x * swell, y * swell, z)
        })
        .collect();
    // Far dots first, so the near ones draw over them.
    points.sort_by(|a, b| a.2.total_cmp(&b.2));
    let radius = center - 16.;
    points
        .into_iter()
        .map(|(x, y, z)| {
            let near = z * 0.5 + 0.5;
            let size = 1.4 + near * 2.6 + stir * 1.2;
            let color = mix(calm, stirred, stir);
            div()
                .absolute()
                .left(px(center + x * radius - size / 2.))
                .top(px(center + y * radius - size / 2.))
                .size(px(size))
                .rounded_full()
                .bg(color.opacity(0.15 + near * 0.75))
        })
        .collect()
}

fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    Hsla {
        h: a.h + (b.h - a.h) * t,
        s: a.s + (b.s - a.s) * t,
        l: a.l + (b.l - a.l) * t,
        a: 1.,
    }
}
