//! The header's sky while the agent works: a faint glow at the right that drifts from
//! violet through rose to a sunset orange as the turn goes on, and stars that come out
//! one by one as it darkens, so how long it has run reads from the corner of the eye.
//! When the turn ends a small star lights up once. Appearance can switch it all off.

use std::time::Duration;

use gpui_kit::{component::ActiveTheme, *};

use crate::workspace::Workspace;

/// The time the sky takes to go from dusk to sunset.
const DUSK_TO_SUNSET: f32 = 15. * 60.;
/// How long the star shows after a turn ends.
const STAR: Duration = Duration::from_millis(1600);

/// The most stars the sky holds, out by the end of dusk.
const STARS: usize = 18;

/// How far the running turn has gone from dusk to night, 0 to 1; none when the shown
/// chat rests or the sky is switched off.
fn late(ws: &Workspace, cx: &App) -> Option<f32> {
    if !crate::ui::theme::prefs(cx).sky {
        return None;
    }
    let chat = ws.shown_chat().filter(|chat| chat.working)?;
    let (began, _) = chat.span(chat.activity_boundaries.len())?;
    let seconds = (crate::pages::chat::timing::now_ms() - began).max(0) as f32 / 1000.;
    Some((seconds / DUSK_TO_SUNSET).min(1.))
}

/// The glow for the header while the shown chat works.
pub fn glow(ws: &Workspace, cx: &App) -> Option<Background> {
    let late = late(ws, cx)?;
    // Violet at 268 degrees to orange at 24, the long way through magenta and rose.
    let hue = (268. + late * 116.) % 360.;
    let color = hsla(hue / 360., 0.7, 0.55, 0.16);
    Some(linear_gradient(
        90.,
        linear_color_stop(color.opacity(0.), 0.35),
        linear_color_stop(color, 1.),
    ))
}

/// The stars over the header's right half, more of them the later it is; each twinkles
/// on its own beat. Their places come from their number, so they stay put.
pub fn stars(ws: &Workspace, cx: &App) -> Option<Div> {
    let late = late(ws, cx)?;
    let out = ((late * STARS as f32).round() as usize).min(STARS);
    let mut sky = div().absolute().inset_0().overflow_hidden();
    for index in 0..out {
        let seed = (index as u32).wrapping_mul(2_654_435_761);
        let x = 0.45 + (seed % 1000) as f32 / 1000. * 0.53;
        let y = 0.15 + (seed / 1000 % 1000) as f32 / 1000. * 0.7;
        let beat = 1800 + (seed % 1700) as u64;
        let size = if seed.is_multiple_of(5) { 2.5 } else { 1.5 };
        sky = sky.child(
            div()
                .absolute()
                .left(relative(x))
                .top(relative(y))
                .size(px(size))
                .rounded_full()
                .bg(hsla(0.12, 0.6, 0.92, 1.))
                .with_animation(
                    SharedString::from(format!("sky-star-{index}")),
                    Animation::new(Duration::from_millis(beat)).repeat(),
                    move |star, delta| {
                        let shine = 0.35 + 0.45 * (delta * std::f32::consts::TAU).sin().abs();
                        star.opacity(shine)
                    },
                ),
        );
    }
    Some(sky)
}

/// A star that brightens and fades once, just after the shown chat's turn ended.
pub fn star(ws: &Workspace, cx: &App) -> Option<impl IntoElement + use<>> {
    if !crate::ui::theme::prefs(cx).sky {
        return None;
    }
    let ended = ws.shown_chat()?.finished_at?;
    if ended.elapsed() > STAR {
        return None;
    }
    let accent = cx.theme().primary;
    let id = SharedString::from(format!("star-{:?}", ended));
    Some(div().text_color(accent).child("\u{2726}").with_animation(
        id,
        Animation::new(STAR).with_easing(ease_in_out),
        |star, delta| star.opacity((delta * std::f32::consts::PI).sin()),
    ))
}
