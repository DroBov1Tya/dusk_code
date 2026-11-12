//! The header's sky while the agent works: a faint glow at the right that drifts from
//! violet through rose to a sunset orange as the turn goes on, so how long it has run
//! reads from the corner of the eye. When the turn ends a small star lights up once.

use std::time::Duration;

use gpui_kit::{component::ActiveTheme, *};

use crate::workspace::Workspace;

/// The time the sky takes to go from dusk to sunset.
const DUSK_TO_SUNSET: f32 = 15. * 60.;
/// How long the star shows after a turn ends.
const STAR: Duration = Duration::from_millis(1600);

/// The glow for the header while the shown chat works.
pub fn glow(ws: &Workspace) -> Option<Background> {
    let chat = ws.shown_chat().filter(|chat| chat.working)?;
    let (began, _) = chat.span(chat.activity_boundaries.len())?;
    let seconds = (crate::pages::chat::timing::now_ms() - began).max(0) as f32 / 1000.;
    let late = (seconds / DUSK_TO_SUNSET).min(1.);
    // Violet at 268 degrees to orange at 24, the long way through magenta and rose.
    let hue = (268. + late * 116.) % 360.;
    let color = hsla(hue / 360., 0.7, 0.55, 0.16);
    Some(linear_gradient(
        90.,
        linear_color_stop(color.opacity(0.), 0.35),
        linear_color_stop(color, 1.),
    ))
}

/// A star that brightens and fades once, just after the shown chat's turn ended.
pub fn star(ws: &Workspace, cx: &App) -> Option<impl IntoElement + use<>> {
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
