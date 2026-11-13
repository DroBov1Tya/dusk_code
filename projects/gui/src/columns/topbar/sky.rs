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

/// How often the sky looks for a meteor, how long its flight takes, and how long the
/// trail it left takes to go out.
const METEOR_EVERY: i64 = 11_000;
const METEOR_FLIGHT: f32 = 240.;
const METEOR_AFTERGLOW: f32 = 620.;
/// The needles its trail is laid from; enough that it reads as one stroke of light.
const METEOR_PARTS: usize = 28;

/// One meteor at one moment.
#[derive(Clone, Copy)]
struct Meteor {
    /// Where it enters at the top, as a share of the width.
    from: f32,
    /// How steeply it falls, in radians below level: shallow, so the trail is long.
    angle: f32,
    /// How far it goes, as a share of the width.
    run: f32,
    /// How much of the way the head has gone, and how much of the trail is left.
    flown: f32,
    left: f32,
    /// One in a few crosses the whole sky, wider and brighter.
    great: bool,
}

/// A falling star, now and then: gone in a quarter of a second, on a shallow path of its
/// own each time, a white point with a long blue trail that thins to nothing and goes
/// out from its far end. The clock picks the moments and the paths, so nothing has to be
/// remembered.
fn meteor() -> impl IntoElement {
    div().absolute().inset_0().with_animation(
        "sky-meteor",
        Animation::new(Duration::from_millis(METEOR_EVERY as u64)).repeat(),
        |sky, _| {
            let now = crate::pages::chat::timing::now_ms();
            let seed = ((now / METEOR_EVERY) as u32).wrapping_mul(2_654_435_761);
            let at = (now % METEOR_EVERY) as f32;
            // The screenshot tool cannot wait for the sky: it names the moment itself.
            #[cfg(feature = "screenshot")]
            let (seed, at) = match std::env::var("DUSK_METEOR").map(|at| at.parse()) {
                Ok(Ok(at)) => (21, at),
                _ => (seed, at),
            };
            // Rare: about one window in three has one.
            if !seed.is_multiple_of(3) || at >= METEOR_FLIGHT + METEOR_AFTERGLOW {
                return sky;
            }
            let unit = |shift: u32| (seed / shift % 1000) as f32 / 1000.;
            let great = (seed / 3).is_multiple_of(7);
            // It slows a little as it burns up.
            let gone = (at / METEOR_FLIGHT).min(1.);
            let meteor = Meteor {
                from: if great { 0.99 } else { 0.66 + unit(3) * 0.32 },
                angle: (if great { 4.5 } else { 6. + unit(3000) * 8. }).to_radians(),
                run: if great { 0.95 } else { 0.24 + unit(7) * 0.2 },
                flown: 1. - (1. - gone) * (1. - gone),
                left: 1. - ((at - METEOR_FLIGHT) / METEOR_AFTERGLOW).clamp(0., 1.),
                great,
            };
            sky.child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| paint_meteor(bounds, meteor, window),
                )
                .absolute()
                .inset_0(),
            )
        },
    )
}

fn paint_meteor(bounds: Bounds<Pixels>, meteor: Meteor, window: &mut Window) {
    let (width, height) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
    let (sin, cos) = meteor.angle.sin_cos();
    // It burns out inside the sky: never further than the header is tall.
    let run = (meteor.run * width).min((height - 4.) / sin);
    let (left, top) = (bounds.origin.x.as_f32(), bounds.origin.y.as_f32());
    let start = (left + meteor.from * width, top + 2.);
    let head = (
        start.0 - cos * run * meteor.flown,
        start.1 + sin * run * meteor.flown,
    );
    // The trail reaches back to where it came in, then goes out from that end.
    let trail = run * meteor.flown * (0.25 + 0.75 * meteor.left);
    let (core, glow) = if meteor.great { (3., 11.) } else { (2., 7.) };
    // The trail is needles laid over each other from the head, each longer and bluer
    // than the last: where many lie it is white, where few reach it is a thin blue, and
    // no needle ends in an edge, so the light has no joints.
    let needle = |reach: f32, wide: f32, color: Hsla, window: &mut Window| {
        let mut path = PathBuilder::fill();
        path.move_to(point(
            px(head.0 + sin * wide / 2.),
            px(head.1 + cos * wide / 2.),
        ));
        path.line_to(point(
            px(head.0 + cos * trail * reach),
            px(head.1 - sin * trail * reach),
        ));
        path.line_to(point(
            px(head.0 - sin * wide / 2.),
            px(head.1 - cos * wide / 2.),
        ));
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    };
    for part in 0..METEOR_PARTS {
        let share = (part + 1) as f32 / METEOR_PARTS as f32;
        let blue = (share * 2.4).min(1.);
        let color = hsla(0.6, blue, 1. - blue * 0.38, meteor.left);
        // The glow under it, wider and all but unseen, then the light itself.
        needle(
            share,
            glow * (0.4 + share),
            color.opacity(0.012 * meteor.left),
            window,
        );
        needle(
            share * share,
            core,
            color.opacity(0.2 * meteor.left),
            window,
        );
    }
    // It is white-hot where it burns: the first quarter of the trail, under the head.
    for part in 1..=6 {
        let white = hsla(0.6, 0.25, 0.97, 0.26 * meteor.left);
        needle(part as f32 * 0.045, core, white, window);
    }
    // The head: a point of white in a halo that thins outward, while it still flies.
    let flying = if meteor.flown < 1. { 1. } else { meteor.left };
    for ring in 0..6 {
        let wide = glow * 1.5 * (1. - ring as f32 / 6.);
        let spot = Bounds::centered_at(point(px(head.0), px(head.1)), size(px(wide), px(wide)));
        let color = hsla(0.6, 1., 0.75, 0.07 * flying);
        window.paint_quad(fill(spot, color).corner_radii(px(wide / 2.)));
    }
    let spot = Bounds::centered_at(
        point(px(head.0), px(head.1)),
        size(px(core * 1.3), px(core * 1.3)),
    );
    window.paint_quad(fill(spot, hsla(0., 0., 1., flying)).corner_radii(px(core)));
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
