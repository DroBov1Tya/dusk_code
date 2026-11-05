//! What shows while the agent works: a sunflower spiral of dots that breathes and turns,
//! a ring of dots on its own orbit turning the other way, and a wave of light running
//! out from the middle, its color moving from the accent toward the text. Every motion
//! is a whole number of turns per cycle, so the loop has no seam.

use std::{f32::consts::TAU, time::Duration};

use gpui_kit::*;

const SIZE: f32 = 28.;
const CORE: usize = 34;
const RING: usize = 12;
/// The golden angle, which spreads the dots evenly on the spiral.
const GOLDEN: f32 = 2.399_963;

pub fn render(id: impl Into<ElementId>, accent: Hsla, light: Hsla) -> impl IntoElement {
    div().relative().size(px(SIZE)).flex_none().with_animation(
        id,
        Animation::new(Duration::from_millis(3600)).repeat(),
        move |figure, phase| figure.children(dots(phase, accent, light)),
    )
}

fn dots(phase: f32, accent: Hsla, light: Hsla) -> Vec<Div> {
    let center = SIZE / 2.;
    // The spiral breathes twice a cycle.
    let breath = 0.86 + 0.14 * (phase * TAU * 2.).sin();
    let core = (0..CORE).map(|index| {
        let reach = (index as f32 + 0.5) / CORE as f32;
        let angle = index as f32 * GOLDEN + phase * TAU;
        let radius = reach.sqrt() * (center - 6.) * breath;
        let wave = ((reach - phase * 2.) * TAU).cos() * 0.5 + 0.5;
        (angle, radius, 1.2 + wave * 1.6, wave)
    });
    let ring = (0..RING).map(|index| {
        let angle = index as f32 / RING as f32 * TAU - phase * TAU;
        // One bright dot chases around the ring.
        let chase = ((index as f32 / RING as f32 + phase) * TAU).cos() * 0.5 + 0.5;
        (angle, center - 1.5, 1.0 + chase * 1.4, chase * chase)
    });
    core.chain(ring)
        .map(|(angle, radius, size, glow)| {
            let color = mix(accent, light, glow * 0.35);
            div()
                .absolute()
                .left(px(center + radius * angle.cos() - size / 2.))
                .top(px(center + radius * angle.sin() - size / 2.))
                .size(px(size))
                .rounded_full()
                .bg(color.opacity(0.2 + glow * 0.8))
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
