//! What shows while the agent works: dots set on a sunflower spiral that turns slowly,
//! a wave of light running out from the middle. Geometry, not a spinner.

use std::{f32::consts::TAU, time::Duration};

use gpui_kit::*;

const SIZE: f32 = 22.;
const DOTS: usize = 21;
/// The golden angle, which spreads the dots evenly on the spiral.
const GOLDEN: f32 = 2.399_963;

pub fn render(id: impl Into<ElementId>, color: Hsla) -> impl IntoElement {
    div()
        .relative()
        .size(px(SIZE))
        .flex_none()
        .with_animation(
            id,
            Animation::new(Duration::from_millis(2400)).repeat(),
            move |figure, phase| figure.children(dots(phase, color)),
        )
}

fn dots(phase: f32, color: Hsla) -> Vec<Div> {
    let center = SIZE / 2.;
    (0..DOTS)
        .map(|index| {
            let reach = (index as f32 + 0.5) / DOTS as f32;
            let angle = index as f32 * GOLDEN + phase * TAU / 3.;
            let radius = reach.sqrt() * (center - 2.);
            // The wave runs outward: a dot is brightest when it passes.
            let wave = ((reach - phase) * TAU).cos() * 0.5 + 0.5;
            let size = 1.6 + wave * 1.6;
            div()
                .absolute()
                .left(px(center + radius * angle.cos() - size / 2.))
                .top(px(center + radius * angle.sin() - size / 2.))
                .size(px(size))
                .rounded_full()
                .bg(color.opacity(0.25 + wave * 0.75))
        })
        .collect()
}
