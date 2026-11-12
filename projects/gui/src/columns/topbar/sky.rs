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

/// Whether the sky is drawn at all; Appearance can switch it off.
pub fn shown(cx: &App) -> bool {
    crate::ui::theme::prefs(cx).sky
}

/// How far the running turn has gone from dusk to night, 0 to 1; none when the shown
/// chat rests or the sky is not shown.
fn late(ws: &Workspace, cx: &App) -> Option<f32> {
    if !shown(cx) {
        return None;
    }
    let chat = ws.shown_chat().filter(|chat| chat.working)?;
    let (began, _) = chat.span(chat.activity_boundaries.len())?;
    let seconds = (crate::pages::chat::timing::now_ms() - began).max(0) as f32 / 1000.;
    // Past sunset it goes on into the night, as long again.
    Some((seconds / DUSK_TO_SUNSET).min(2.))
}

/// The glow for the header while the shown chat works.
pub fn glow(ws: &Workspace, cx: &App) -> Option<Background> {
    let late = late(ws, cx)?;
    // Violet at 268 degrees to orange at 24, the long way through magenta and rose; a
    // turn that runs on past sunset sees the orange go back down through red and violet
    // into a deep blue, darker and a little fuller, as the night comes.
    let night = (late - 1.).max(0.);
    let hue = if night > 0. {
        (24. - night * 144.).rem_euclid(360.)
    } else {
        (268. + late * 116.) % 360.
    };
    let color = hsla(hue / 360., 0.7, 0.55 - night * 0.17, 0.16 + night * 0.06);
    Some(linear_gradient(
        90.,
        linear_color_stop(color.opacity(0.), 0.35),
        linear_color_stop(color, 1.),
    ))
}

/// The stars over the header's right half, more of them the later it is; each twinkles
/// on its own beat. Their places come from their number, so they stay put.
pub fn stars(ws: &Workspace, cx: &App) -> Option<Div> {
    let late = late(ws, cx)?.min(1.);
    let out = ((late * STARS as f32).round() as usize).min(STARS);
    let dark = cx.theme().is_dark();
    let mut sky = div().absolute().inset_0().overflow_hidden();
    // A meteor needs a night to fall through: it waits for the first stars.
    if out >= 3 && dark {
        sky = sky.child(meteor());
    }
    for index in 0..out {
        let seed = (index as u32).wrapping_mul(2_654_435_761);
        let x = 0.45 + (seed % 1000) as f32 / 1000. * 0.53;
        let y = 0.15 + (seed / 1000 % 1000) as f32 / 1000. * 0.7;
        let beat = 1800 + (seed % 1700) as u64;
        sky = sky.child(
            star_shape(seed, dark)
                .left(relative(x))
                .top(relative(y))
                .with_animation(
                    SharedString::from(format!("sky-star-{index}")),
                    Animation::new(Duration::from_millis(beat)).repeat(),
                    move |star, delta| {
                        let shine = 0.3 + 0.5 * (delta * std::f32::consts::TAU).sin().abs();
                        star.opacity(shine)
                    },
                ),
        );
    }
    Some(sky)
}

/// How often the sky looks for a meteor, and how long one takes to fall and fade.
const METEOR_EVERY: i64 = 11_000;
const METEOR_FALL: f32 = 950.;
/// The dots of its trail, head first.
const METEOR_TRAIL: usize = 14;

/// A falling star, now and then: fast, always downward, on a path of its own each time,
/// white at the head and bright blue behind, its trail thinning out after it has gone.
/// The clock picks the moments and the paths, so nothing has to be remembered.
fn meteor() -> impl IntoElement {
    div().absolute().inset_0().with_animation(
        "sky-meteor",
        Animation::new(Duration::from_millis(METEOR_EVERY as u64)).repeat(),
        |sky, _| {
            let now = crate::pages::chat::timing::now_ms();
            let seed = ((now / METEOR_EVERY) as u32).wrapping_mul(2_654_435_761);
            let at = (now % METEOR_EVERY) as f32 / METEOR_FALL;
            // Rare: about one window in three has one.
            if !seed.is_multiple_of(3) || at >= 1. {
                return sky;
            }
            let from = 0.5 + (seed / 3 % 1000) as f32 / 1000. * 0.42;
            let lean = ((seed / 3000 % 1000) as f32 / 1000. - 0.5) * 0.12;
            // The head crosses in the first half; the rest is the trail going out.
            let head = (at / 0.5).min(1.);
            let fade = 1. - ((at - 0.5) / 0.5).max(0.);
            sky.children((0..METEOR_TRAIL).filter_map(move |index| {
                let behind = index as f32 / METEOR_TRAIL as f32;
                let along = head - behind * 0.55;
                if !(0. ..=1.).contains(&along) {
                    return None;
                }
                let size = 2.2 - behind * 1.6;
                let color = hsla(0.6, behind.min(1.), 1. - behind * 0.38, 1.);
                Some(
                    div()
                        .absolute()
                        .left(relative(from + lean * along))
                        .top(relative(along * 1.1 - 0.05))
                        .size(px(size))
                        .rounded_full()
                        .bg(color.opacity((1. - behind) * fade)),
                )
            }))
        },
    )
}

/// One star: a soft point in a haze, and on the larger ones four short rays that thin
/// out to nothing. A rare one leans a little to blue, green or red.
fn star_shape(seed: u32, dark: bool) -> Div {
    // Light stars on a dark header; on a light theme they are drawn in ink instead.
    let (light, soft) = if dark { (0.87, 0.92) } else { (0.42, 0.36) };
    let color = match seed / 7 % 9 {
        0 => hsla(0.6, 0.55, light, 1.),
        1 => hsla(0.42, 0.4, light, 1.),
        2 => hsla(0.99, 0.55, light, 1.),
        _ => hsla(if dark { 0.12 } else { 0.72 }, 0.6, soft, 1.),
    };
    let large = seed.is_multiple_of(3);
    let core = if large { 2. } else { 1.5 };
    let reach = if large { 5. } else { 0. };
    let side = core + reach * 2.;
    let middle = reach + core / 2. - 0.5;
    let haze = BoxShadow {
        color: color.opacity(0.55),
        offset: point(px(0.), px(0.)),
        blur_radius: px(if large { 5. } else { 3. }),
        spread_radius: px(0.5),
        inset: false,
    };
    let mut star = div().absolute().size(px(side)).child(
        div()
            .absolute()
            .left(px(reach))
            .top(px(reach))
            .size(px(core))
            .rounded_full()
            .bg(color.opacity(0.9))
            .shadow(vec![haze]),
    );
    if !large {
        return star;
    }
    // Each ray starts at the point and fades away from it: angle, left, top, wide, tall.
    let rays = [
        (90., reach + core, middle, reach, 1.),
        (270., 0., middle, reach, 1.),
        (0., middle, 0., 1., reach),
        (180., middle, reach + core, 1., reach),
    ];
    for (angle, left, top, wide, tall) in rays {
        star = star.child(
            div()
                .absolute()
                .left(px(left))
                .top(px(top))
                .w(px(wide))
                .h(px(tall))
                .bg(linear_gradient(
                    angle,
                    linear_color_stop(color.opacity(0.7), 0.),
                    linear_color_stop(color.opacity(0.), 1.),
                )),
        );
    }
    star
}

/// A star that brightens and fades once, just after the shown chat's turn ended.
pub fn star(ws: &Workspace, cx: &App) -> Option<impl IntoElement + use<>> {
    if !shown(cx) {
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
