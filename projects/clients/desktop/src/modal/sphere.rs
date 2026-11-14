//! The arbiter as a sphere of dots: points spread evenly on a sphere turn very slowly,
//! the near ones larger and brighter, and the whole mass breathes. Each request and
//! each answer sends a resonant wave from the center: a compression front runs out
//! across the sphere, the dots pull in as it passes and spring back, and the sphere
//! rings for a moment before it settles.

use std::{
    f32::consts::TAU,
    time::{Duration, Instant},
};

use gpui_kit::*;

const SIZE: f32 = 200.;
const DOTS: usize = 160;
/// The golden angle spreads the points evenly over the sphere.
const GOLDEN: f32 = 2.399_963;
/// One turn of the sphere; the slow breath fits six times into it.
const TURN: Duration = Duration::from_secs(48);
const BREATHS_PER_TURN: f32 = 6.;
/// How long a wave takes to cross the sphere and the ringing to fade.
const WAVE: f32 = 1.6;
const RING: f32 = 2.6;

pub fn render(
    id: impl Into<ElementId>,
    last: Option<Instant>,
    calm: Hsla,
    stirred: Hsla,
) -> impl IntoElement {
    div().relative().size(px(SIZE)).flex_none().with_animation(
        id,
        Animation::new(TURN).repeat(),
        move |sphere, phase| {
            let since = last.map(|at| at.elapsed().as_secs_f32());
            sphere.children(dots(phase, since, calm, stirred))
        },
    )
}

/// How far each dot is pulled toward the center, 0 to 1, and the sphere's scale, for a
/// wave sent `since` seconds ago.
struct Wave {
    front: f32,
    strength: f32,
    ring: f32,
}

impl Wave {
    fn at(since: Option<f32>) -> Self {
        let Some(since) = since.filter(|since| *since < RING) else {
            return Self {
                front: f32::INFINITY,
                strength: 0.,
                ring: 0.,
            };
        };
        let fade = 1. - since / RING;
        Self {
            // The front starts inside and runs past the rim.
            front: since / WAVE * 1.3 - 0.1,
            strength: (1. - since / WAVE).max(0.),
            // A damped ring of the whole mass after the front passed.
            ring: (since * TAU * 1.4).sin() * 0.06 * fade * fade,
        }
    }

    /// The pull on a dot `distance` from the center, as seen.
    fn pull(&self, distance: f32) -> f32 {
        let gap = (distance - self.front) / 0.16;
        0.24 * self.strength * (-gap * gap).exp()
    }
}

fn dots(phase: f32, since: Option<f32>, calm: Hsla, stirred: Hsla) -> Vec<Div> {
    let center = SIZE / 2.;
    let turn = phase * TAU;
    let breath = 1. + 0.015 * (turn * BREATHS_PER_TURN).sin();
    let wave = Wave::at(since);
    let mut points: Vec<(f32, f32, f32, f32)> = (0..DOTS)
        .map(|index| {
            // A Fibonacci sphere: even latitude steps, golden-angle longitude steps.
            let y = 1. - 2. * (index as f32 + 0.5) / DOTS as f32;
            let ring = (1. - y * y).sqrt();
            let longitude = index as f32 * GOLDEN + turn;
            let (x, z) = (ring * longitude.cos(), ring * longitude.sin());
            let pull = wave.pull((x * x + y * y).sqrt());
            let scale = breath * (1. + wave.ring) * (1. - pull);
            (x * scale, y * scale, z, pull)
        })
        .collect();
    // Far dots first, so the near ones draw over them.
    points.sort_by(|a, b| a.2.total_cmp(&b.2));
    let radius = center - 16.;
    points
        .into_iter()
        .map(|(x, y, z, pull)| {
            let near = z * 0.5 + 0.5;
            // A dot the front passes swells and warms.
            let lit = (pull / 0.24).min(1.);
            let size = 1.4 + near * 2.6 + lit * 2.;
            let color = mix(calm, stirred, lit);
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

#[cfg(test)]
mod tests {
    use super::Wave;

    #[test]
    fn a_wave_passes_and_the_sphere_settles() {
        let early = Wave::at(Some(0.2));
        let later = Wave::at(Some(1.0));
        assert!(early.front < later.front, "the front runs outward");
        assert!(
            early.pull(early.front) > later.pull(later.front),
            "and weakens"
        );
        assert_eq!(Wave::at(Some(5.)).pull(0.5), 0.);
        assert_eq!(Wave::at(None).ring, 0.);
    }
}
