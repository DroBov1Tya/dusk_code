//! Fireflies over the message box while subagents work: one warm light for each agent at
//! work, wandering and glowing on its own beat, so how many are busy reads without a
//! number. They take no clicks and go out when the agents are done. The sky's switch in
//! Appearance turns them off too.

use std::{f32::consts::TAU, time::Duration};

use gpui_kit::*;

use crate::workspace::Workspace;

/// More agents than this still show this many lights.
const MOST: usize = 12;
/// The band over the box they wander in.
const BAND: f32 = 84.;

pub fn render(ws: &Workspace, cx: &App) -> Option<impl IntoElement + use<>> {
    if !crate::ui::theme::prefs(cx).sky {
        return None;
    }
    let working = ws
        .shown_chat()?
        .agents
        .values()
        .filter(|agent| agent.working)
        .count()
        .min(MOST);
    if working == 0 {
        return None;
    }
    Some(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(BAND))
            .with_animation(
                "fireflies",
                Animation::new(Duration::from_secs(24))
                    .repeat()
                    .with_max_fps(24.),
                move |band, phase| band.children((0..working).map(|index| firefly(index, phase))),
            ),
    )
}

/// One light: its path is two slow turns at different rates, so it never repeats within
/// sight; it glows and dims a few times a cycle.
fn firefly(index: usize, phase: f32) -> Div {
    let seed = (index as u32 + 1).wrapping_mul(2_654_435_761);
    let part = |shift: u32| (seed >> shift) % 1000;
    let home = 0.12 + part(0) as f32 / 1000. * 0.76;
    let (across, along) = (1 + part(8) % 3, 2 + part(12) % 3);
    let offset = part(4) as f32 / 1000.;
    let x = home + ((phase * across as f32 + offset) * TAU).sin() * 0.07;
    let y = 0.5 + ((phase * along as f32 + offset * 2.) * TAU).cos() * 0.34;
    let glow = ((phase * (5 + part(16) % 4) as f32 + offset) * TAU).sin() * 0.5 + 0.5;
    let color = hsla(0.13, 0.95, 0.7, 1.);
    div()
        .absolute()
        .left(relative(x))
        .top(relative(y))
        .size(px(2.5))
        .rounded_full()
        .bg(color.opacity(0.25 + glow * 0.75))
        .shadow(vec![BoxShadow {
            color: color.opacity(0.15 + glow * 0.45),
            offset: point(px(0.), px(0.)),
            blur_radius: px(4. + glow * 5.),
            spread_radius: px(glow * 1.5),
            inset: false,
        }])
}
