//! What shows while the agent works: a sunflower spiral of dots that breathes and turns
//! around a glowing heart, a wave of light running out from the middle, and two comets on
//! one orbit turning the other way, each with a tail that thins out behind it. Its colors
//! move from the accent toward the text. Every motion is a whole number of turns per
//! cycle, so the loop has no seam.

use std::{f32::consts::TAU, time::Duration};

use gpui_kit::*;

const SIZE: f32 = 28.;
const CORE: usize = 34;
/// The dots of one comet, head first.
const TAIL: usize = 9;
/// The golden angle, which spreads the dots evenly on the spiral.
const GOLDEN: f32 = 2.399_963;

pub fn render(id: impl Into<ElementId>, accent: Hsla, light: Hsla) -> impl IntoElement {
    div().relative().size(px(SIZE)).flex_none().with_animation(
        id,
        Animation::new(Duration::from_millis(3600)).repeat(),
        move |figure, phase| {
            figure
                .child(heart(phase, accent))
                .children(dots(phase, accent, light))
        },
    )
}

/// The glow in the middle, swelling with the spiral's breath.
fn heart(phase: f32, accent: Hsla) -> Div {
    let beat = (phase * TAU * 2.).sin() * 0.5 + 0.5;
    let size = 2.;
    div()
        .absolute()
        .left(px(SIZE / 2. - size / 2.))
        .top(px(SIZE / 2. - size / 2.))
        .size(px(size))
        .rounded_full()
        .bg(accent.opacity(0.5 + beat * 0.4))
        .shadow(vec![BoxShadow {
            color: accent.opacity(0.25 + beat * 0.3),
            offset: point(px(0.), px(0.)),
            blur_radius: px(5. + beat * 4.),
            spread_radius: px(1. + beat * 1.5),
            inset: false,
        }])
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
    // Two comets half a turn apart; each dot of a tail trails the one before it.
    let comets = (0..2 * TAIL).map(|index| {
        let (comet, place) = (index / TAIL, index % TAIL);
        let fade = 1. - place as f32 / TAIL as f32;
        let angle = comet as f32 * TAU / 2. - phase * TAU + place as f32 * 0.17;
        (angle, center - 1.5, 0.7 + fade * 1.9, fade * fade)
    });
    core.chain(comets)
        .map(|(angle, radius, size, glow)| {
            let color = mix(accent, light, glow * 0.35);
            div()
                .absolute()
                .left(px(center + radius * angle.cos() - size / 2.))
                .top(px(center + radius * angle.sin() - size / 2.))
                .size(px(size))
                .rounded_full()
                .bg(color.opacity(0.12 + glow * 0.88))
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
