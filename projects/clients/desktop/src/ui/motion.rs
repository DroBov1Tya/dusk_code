//! Motion that answers what a person did or what just arrived: short, eased out, played
//! once and never while idle. Each helper is a oneshot `with_animation`, which gpui draws
//! at its end state when the system asks for less motion, so none of them checks that.
//!
//! A oneshot plays when an element with its ID first appears; a new ID plays it again.

use std::time::{Duration, Instant};

use gpui_kit::*;

/// A page taking the place of another.
pub const SWITCH: Duration = Duration::from_millis(150);
/// A message or a block arriving in the chat.
pub const ARRIVE: Duration = Duration::from_millis(200);
/// A moved row settling into its new place.
pub const SETTLE: Duration = Duration::from_millis(220);

/// Fast at first, gentle at the end: the eye sees the answer at once and the stop is soft.
pub fn ease_out(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// A page that fades in over the one it replaced. gpui keeps no element after it is
/// gone, so the old page cannot fade out under it; at 150 ms that reads as a cross-fade.
pub fn switch<E>(id: impl Into<ElementId>, page: E) -> AnimationElement<E>
where
    E: IntoElement + Styled + 'static,
{
    page.with_animation(
        id,
        Animation::new(SWITCH).with_easing(ease_out),
        |page, t| page.opacity(t),
    )
}

/// Something new in a list, first drawn at `since`: it fades in and rises the last 4 px.
/// A relative offset, so the rows around it do not move while it plays. A list drops a
/// row scrolled out of sight and builds it again later; by then `since` is old and the
/// row stands still, so scrolling never replays it.
pub fn arrive<E>(id: impl Into<ElementId>, row: E, since: Instant) -> AnimationElement<E>
where
    E: IntoElement + Styled + 'static,
{
    let fresh = since.elapsed() < ARRIVE;
    row.with_animation(
        id,
        Animation::new(ARRIVE).with_easing(ease_out),
        move |row, t| {
            let t = if fresh { t } else { 1. };
            row.opacity(t).relative().top(px((1. - t) * 4.))
        },
    )
}

/// How far a motion that began at `since` has come, eased, while it runs: the next frame
/// is asked for. `None` once it is done or when the system asks for less motion, so a
/// still element costs nothing. For an element that must keep its place in the tree,
/// where a wrapper with a new ID would lose its state.
pub fn progress(since: Instant, length: Duration, window: &Window, cx: &App) -> Option<f32> {
    let t = since.elapsed().as_secs_f32() / length.as_secs_f32();
    if cx.reduce_motion() || t >= 1. {
        return None;
    }
    window.request_animation_frame();
    Some(ease_out(t))
}
