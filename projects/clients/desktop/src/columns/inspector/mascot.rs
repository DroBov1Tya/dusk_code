//! Sundown: the mascot of duskcode, one for each subagent. A small sun half set on its
//! horizon, with two eyes and a faint reflection: the name of the app made visible. Every
//! activity is told by its own rhythm, done hops clear of the horizon and failed sets below
//! it. Ported from the owner's prototype (`dusk_notes/mascot/prototype.html`): one pose
//! function in units of a 100-wide box, drawn as rounded quads under one rectangular clip.

use std::{
    f32::consts::{PI, TAU},
    hash::{DefaultHasher, Hash, Hasher},
    time::Instant,
};

use dusk_protocol::{agent::Doing, thread::AgentState};
use gpui_kit::*;

/// The side of the box in `unit`s, the size the callers have laid out for.
const SIDE: f32 = 22.;
/// The sun's radius in box units.
const R: f32 = 24.;
/// A new activity blends in from the last pose over this long.
const BLEND: f32 = 0.28;
/// Done and failed have played and hold still after this long.
const SETTLED: f32 = 1.4;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Thinking,
    Writing,
    Searching,
    Reading,
    Planning,
    Running,
    Done,
    Failed,
}

impl Kind {
    fn of(state: &AgentState) -> Self {
        match state {
            AgentState::Thinking => Self::Thinking,
            AgentState::Writing => Self::Writing,
            AgentState::Tool(name) => match Doing::of_tool(name) {
                Doing::Searching => Self::Searching,
                Doing::Reading => Self::Reading,
                Doing::Planning => Self::Planning,
                Doing::Thinking => Self::Thinking,
                Doing::Writing => Self::Writing,
                Doing::Running => Self::Running,
            },
            AgentState::Done => Self::Done,
            AgentState::Failed => Self::Failed,
        }
    }

    /// Played once, then still.
    fn once(self) -> bool {
        matches!(self, Self::Done | Self::Failed)
    }
}

/// Everything that moves, in box units (0..100) or 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    cx: f32,
    cy: f32,
    sx: f32,
    sy: f32,
    eye_o: f32,
    eye_w: f32,
    gx: f32,
    gy: f32,
    happy: f32,
    sad: f32,
    glow: f32,
    desat: f32,
    ring_r: f32,
    ring_o: f32,
    spin: f32,
    head: f32,
    head_mix: f32,
    orbit_o: f32,
    orbit_a: f32,
    stars: [f32; 3],
    ref_o: f32,
    ref_w: f32,
}

const BASE: Pose = Pose {
    cx: 50.,
    cy: 54.,
    sx: 1.,
    sy: 1.,
    eye_o: 1.,
    eye_w: 1.,
    gx: 0.,
    gy: 0.,
    happy: 0.,
    sad: 0.,
    glow: 0.6,
    desat: 0.,
    ring_r: 34.,
    ring_o: 0.,
    spin: 0.,
    head: 0.,
    head_mix: 0.,
    orbit_o: 0.,
    orbit_a: 0.,
    stars: [0.; 3],
    ref_o: 0.5,
    ref_w: 1.,
};

impl Pose {
    fn blend(&self, to: &Pose, k: f32) -> Pose {
        let l = |a: f32, b: f32| lerp(a, b, k);
        Pose {
            cx: l(self.cx, to.cx),
            cy: l(self.cy, to.cy),
            sx: l(self.sx, to.sx),
            sy: l(self.sy, to.sy),
            eye_o: l(self.eye_o, to.eye_o),
            eye_w: l(self.eye_w, to.eye_w),
            gx: l(self.gx, to.gx),
            gy: l(self.gy, to.gy),
            happy: l(self.happy, to.happy),
            sad: l(self.sad, to.sad),
            glow: l(self.glow, to.glow),
            desat: l(self.desat, to.desat),
            ring_r: l(self.ring_r, to.ring_r),
            ring_o: l(self.ring_o, to.ring_o),
            spin: l(self.spin, to.spin),
            head: l(self.head, to.head),
            head_mix: l(self.head_mix, to.head_mix),
            orbit_o: l(self.orbit_o, to.orbit_o),
            orbit_a: l(self.orbit_a, to.orbit_a),
            stars: [0, 1, 2].map(|i| l(self.stars[i], to.stars[i])),
            ref_o: l(self.ref_o, to.ref_o),
            ref_w: l(self.ref_w, to.ref_w),
        }
    }
}

fn clamp01(x: f32) -> f32 {
    x.clamp(0., 1.)
}

fn lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k
}

fn frac(x: f32) -> f32 {
    x - x.floor()
}

fn eo(k: f32) -> f32 {
    1. - (1. - k).powi(3)
}

fn ei(k: f32) -> f32 {
    k * k * k
}

fn eio(k: f32) -> f32 {
    if k < 0.5 {
        4. * k * k * k
    } else {
        1. - (-2. * k + 2.).powi(3) / 2.
    }
}

/// A repeatable pseudo-random 0..1 for a number; f64 so the fraction keeps its bits.
fn noise(n: f32) -> f32 {
    let x = ((n as f64 * 127.1 + 311.7).sin() * 43758.5453).fract();
    (if x < 0. { x + 1. } else { x }) as f32
}

fn osc(t: f32, period: f32) -> f32 {
    (t / period * TAU).sin()
}

/// Keys of (time, value, easing of the segment that ends there).
type Key = (f32, f32, fn(f32) -> f32);

fn keys(x: f32, keys: &[Key]) -> f32 {
    if x <= keys[0].0 {
        return keys[0].1;
    }
    for pair in keys.windows(2) {
        let ((t0, v0, _), (t1, v1, ease)) = (pair[0], pair[1]);
        if x <= t1 {
            return lerp(v0, v1, ease((x - t0) / (t1 - t0)));
        }
    }
    keys[keys.len() - 1].1
}

/// A blink every few seconds, sometimes a double one: 0 open, 1 shut.
fn blink(t: f32, seed: f32) -> f32 {
    let period = 4.3;
    let tt = t + seed * 1.7;
    let k = (tt / period).floor();
    let local = tt - k * period;
    let at = 0.6 + noise(k + seed) * 2.6;
    let dur = 0.17;
    let tri = |d: f32| {
        if (0. ..dur).contains(&d) {
            1. - (d / dur * 2. - 1.).abs()
        } else {
            0.
        }
    };
    let mut v = tri(local - at);
    if noise(k * 3. + seed) > 0.72 {
        v = v.max(tri(local - at - 0.3));
    }
    v
}

/// Glances: a hold, then a quick move to a new point.
fn saccade(t: f32, period: f32, seed: f32, amp: f32) -> f32 {
    let k = (t / period).floor();
    let l = t / period - k;
    let a = (noise(k - 1. + seed * 9.) * 2. - 1.) * amp;
    let b = (noise(k + seed * 9.) * 2. - 1.) * amp;
    lerp(a, b, eo(clamp01(l / 0.15)))
}

/// Darts that alternate left and right.
fn dart(t: f32, period: f32, amp: f32, seed: f32) -> f32 {
    let k = (t / period).floor();
    let l = t / period - k;
    let target = |j: f32| {
        let side = if (j as i64) & 1 == 1 { 1. } else { -1. };
        side * amp * (0.55 + 0.45 * noise(j + seed))
    };
    lerp(target(k - 1.), target(k), eo(clamp01(l / 0.18)))
}

/// Along a line from a to b, then a quick return.
fn sweep(t: f32, period: f32, a: f32, b: f32, ret: f32) -> f32 {
    let l = frac(t / period);
    if l < 1. - ret {
        lerp(a, b, l / (1. - ret))
    } else {
        lerp(b, a, eo((l - (1. - ret)) / ret))
    }
}

/// 0 along the line, a bump during the return.
fn sweep_return(t: f32, period: f32, ret: f32) -> f32 {
    let l = frac(t / period);
    if l < 1. - ret {
        0.
    } else {
        (PI * (l - (1. - ret)) / ret).sin()
    }
}

/// The ith of three stars: in one by one, out together.
fn star(l: f32, i: f32) -> f32 {
    clamp01((l - (0.1 + i * 0.18)) / 0.08) * (1. - clamp01((l - 0.84) / 0.12))
}

/// The pose at `t` seconds of the mascot's life and `ts` seconds into its activity.
fn pose(kind: Kind, t: f32, ts: f32, seed: f32) -> Pose {
    let mut p = BASE;
    let b = blink(t, seed);
    match kind {
        // Breathes, eyes up and glancing, three motes orbit behind and before it.
        Kind::Thinking => {
            let k = osc(t, 3.2);
            p.cy = 54. + k;
            p.sx = 1. + k * 0.02;
            p.sy = 1. - k * 0.02;
            p.gy = -4.;
            p.gx = saccade(t, 2.3, seed, 4.);
            p.eye_o = 0.85 * (1. - b);
            p.orbit_o = 1.;
            p.orbit_a = t * TAU / 2.8;
            p.glow = 0.7;
        }
        // A quick bob with a squash at each landing, eyes low along a line.
        Kind::Writing => {
            let hop = (PI * frac(t / 0.42)).sin();
            let land = (1. - hop).powi(6);
            p.cy = 54. - 1.8 * hop;
            p.sy = 1. - 0.05 * land;
            p.sx = 1. + 0.05 * land;
            p.gy = 3.;
            p.gx = sweep(t, 1.5, -4., 4., 0.15);
            p.eye_o = 0.7 * (1. - b);
            p.glow = 0.72;
            p.ref_w = 1. + 0.08 * hop;
        }
        // The eyes dart, the body follows a beat later, a light sweeps the rays.
        Kind::Searching => {
            p.gx = dart(t, 0.75, 5., seed);
            p.cx = 50. + dart(t - 0.09, 0.75, 5., seed) * 0.35;
            p.eye_o = 1.08 * (1. - b * 0.8);
            p.eye_w = 1.1;
            p.ring_o = 1.;
            p.head_mix = 1.;
            p.head = t * TAU / 1.5;
            p.glow = 0.7;
        }
        // The eyes trace a line, then snap back with a blink and a nod.
        Kind::Reading => {
            let r = sweep_return(t, 2.1, 0.18);
            p.gy = 2.5;
            p.gx = sweep(t, 2.1, -4.5, 4.5, 0.18);
            p.eye_o = 0.78 * (1. - b.max(r));
            p.cy = 54. + r * 0.8;
        }
        // Eyes shut, a slow deep breath, three stars come out one by one.
        Kind::Planning => {
            let k = osc(t, 4.6);
            let l = frac(t / 3.6);
            p.cy = 55. + k * 1.4;
            p.sx = 1. + k * 0.025;
            p.sy = 1. - k * 0.025;
            p.eye_o = 0.14;
            p.glow = 0.55 + 0.2 * (k * 0.5 + 0.5);
            p.stars = [star(l, 0.), star(l, 1.), star(l, 2.)];
        }
        // Risen a little, the rays turn and pulse, the reflection shimmers.
        Kind::Running => {
            p.cy = 50. + osc(t, 1.8) * 0.6;
            p.ring_o = 1.;
            p.spin = t * TAU / 3.2;
            p.ring_r = 33. + osc(t, 0.9) * 1.3;
            p.eye_o = 0.55 * (1. - b);
            p.glow = 0.9 + 0.1 * osc(t, 0.9);
            p.ref_o = 0.9;
            p.ref_w = 1. + 0.15 * osc(t, 0.6);
        }
        // A squash, a hop clear of the horizon, a landing, happy eyes and a burst of rays.
        Kind::Done => {
            p.cy = keys(
                ts,
                &[
                    (0., 54., eio),
                    (0.12, 56.5, eio),
                    (0.38, 38., eo),
                    (0.62, 54.5, ei),
                    (0.74, 51.5, eo),
                    (0.9, 53., eio),
                ],
            );
            p.sy = keys(
                ts,
                &[
                    (0., 1., eio),
                    (0.12, 0.9, eio),
                    (0.24, 1.08, eio),
                    (0.38, 1., eio),
                    (0.6, 1., eio),
                    (0.64, 0.9, eio),
                    (0.76, 1.03, eio),
                    (0.9, 1., eio),
                ],
            );
            p.sx = 2. - p.sy;
            p.happy = keys(ts, &[(0.08, 0., eio), (0.28, 1., eio)]);
            p.eye_w = 1.15;
            p.gy = -1.;
            p.ring_o = keys(ts, &[(0.34, 0., eio), (0.4, 1., eio), (1., 0., eio)]);
            p.ring_r = keys(ts, &[(0.36, 26., eio), (1., 46., eo)]);
            p.glow = keys(ts, &[(0., 0.6, eio), (0.4, 1., eio), (1.4, 0.8, eio)]);
        }
        // A shiver, then it sets, greys out and its lids grow heavy.
        Kind::Failed => {
            let shiver = if ts < 0.35 {
                (ts * TAU * 13.).sin() * 2.2 * (1. - ts / 0.35)
            } else {
                0.
            };
            p.cx = 50. + shiver;
            p.cy = keys(ts, &[(0.3, 54., eio), (1., 66., eio)]);
            p.desat = keys(ts, &[(0.15, 0., eio), (0.8, 1., eio)]);
            p.glow = keys(ts, &[(0.1, 0.6, eio), (0.8, 0.12, eio)]);
            p.sad = keys(ts, &[(0.05, 0., eio), (0.3, 1., eio)]);
            p.eye_o = keys(ts, &[(0.05, 1., eio), (0.3, 0.6, eio)]);
            p.gy = keys(ts, &[(0.2, 0., eio), (0.6, 2.5, eio)]);
            p.ref_o = keys(ts, &[(0.3, 0.5, eio), (1., 0.15, eio)]);
        }
    }
    p
}

fn mix(a: Rgba, b: Rgba, k: f32) -> Rgba {
    Rgba {
        r: lerp(a.r, b.r, k),
        g: lerp(a.g, b.g, k),
        b: lerp(a.b, b.b, k),
        a: lerp(a.a, b.a, k),
    }
}

/// Draws box units into the canvas.
struct Pen<'a> {
    window: &'a mut Window,
    origin: Point<Pixels>,
    scale: f32,
}

impl Pen<'_> {
    fn bounds(&self, x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(
                self.origin.x + px(x * self.scale),
                self.origin.y + px(y * self.scale),
            ),
            size: size(px(w.max(0.) * self.scale), px(h.max(0.) * self.scale)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: Rgba, opacity: f32) {
        let opacity = clamp01(opacity);
        if opacity <= 0. {
            return;
        }
        let bounds = self.bounds(x, y, w, h);
        let color = Hsla::from(Rgba {
            a: color.a * opacity,
            ..color
        });
        self.window
            .paint_quad(fill(bounds, color).corner_radii(px(r * self.scale)));
    }

    /// A circle, or a pill when w and h differ, around its center.
    fn dot(&mut self, cx: f32, cy: f32, w: f32, h: f32, color: Rgba, opacity: f32) {
        self.rect(
            cx - w / 2.,
            cy - h / 2.,
            w,
            h,
            w.min(h) / 2.,
            color,
            opacity,
        );
    }
}

fn paint(p: &Pose, tint: Rgba, bounds: Bounds<Pixels>, window: &mut Window) {
    let bg = rgb(0x111318);
    let ink = rgb(0x14161c);
    let grey = rgb(0x7a7f8a);
    let white = rgb(0xffffff);
    let t0 = mix(tint, grey, p.desat * 0.85);
    let body = mix(t0, bg, p.desat * 0.25);
    let scale = f32::from(bounds.size.width) / 100.;
    let mut pen = Pen {
        window,
        origin: bounds.origin,
        scale,
    };
    let orbit: Vec<(f32, f32, bool)> = (0..3)
        .map(|i| {
            let a = p.orbit_a + i as f32 * TAU / 3.;
            (
                p.cx + 33. * a.cos(),
                p.cy - 6. + 10.5 * a.sin(),
                a.sin() > 0.,
            )
        })
        .collect();
    // The horizon: what is under the line is set, so it is cut off.
    let sky = pen.bounds(0., 0., 100., 74.);
    pen.window
        .with_content_mask(Some(ContentMask { bounds: sky }), |window| {
            let mut pen = Pen {
                window,
                origin: bounds.origin,
                scale,
            };
            for &(x, y, front) in &orbit {
                let o = p.orbit_o * if front { 0. } else { 0.5 };
                pen.dot(x, y, 4., 4., body, o);
            }
            for i in 0..8 {
                let a = p.spin + i as f32 * TAU / 8. - PI / 2.;
                let highlight = 0.12 + 0.88 * (a - p.head).cos().max(0.).powi(4);
                let (x, y) = (p.cx + p.ring_r * a.cos(), p.cy + p.ring_r * a.sin());
                pen.dot(
                    x,
                    y,
                    4.6,
                    4.6,
                    body,
                    p.ring_o * lerp(1., highlight, p.head_mix),
                );
            }
            let (w, h) = (2. * R * p.sx, 2. * R * p.sy);
            let sun = pen.bounds(p.cx - w / 2., p.cy - h / 2., w, h);
            let radius = px(w.min(h) / 2. * scale);
            // gpui's blur radius is a sigma; the prototype's CSS blur is twice that.
            pen.window.paint_drop_shadows(
                sun,
                Corners::all(radius),
                &[BoxShadow {
                    color: Hsla::from(Rgba {
                        a: 0.5 * p.glow,
                        ..t0
                    }),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(9. * p.glow * scale),
                    spread_radius: px(1.5 * p.glow * scale),
                    inset: false,
                }],
            );
            pen.dot(p.cx, p.cy, w, h, body, 1.);
            // Happy and heavy eyes are body-colored covers over plain eyes: a circle from
            // below leaves a crescent, a lid from above leaves a tired slit.
            for side in [-1., 1.] {
                let ex = p.cx + side * 8.5 * p.sx + p.gx;
                let ey = p.cy - 3. * p.sy + p.gy;
                let w = 7. * p.eye_w * p.sx;
                let h = (11. * p.eye_o * p.sy).max(1.8);
                pen.rect(ex - w / 2., ey - h / 2., w, h, w.min(h) / 2., ink, 1.);
                if p.happy > 0.01 {
                    pen.dot(ex, ey + lerp(11., 3.2, p.happy), 11., 11., body, 1.);
                }
                if p.sad > 0.01 {
                    let lid = 6. + h * 0.5 * p.sad;
                    pen.rect(
                        ex - w / 2. - 1.,
                        ey - h / 2. - 6.,
                        w + 2.,
                        lid,
                        1.,
                        body,
                        1.,
                    );
                }
            }
            for &(x, y, front) in &orbit {
                let o = p.orbit_o * if front { 1. } else { 0. };
                pen.dot(x, y, 4., 4., mix(body, white, 0.25), o);
            }
        });
    for (i, o) in p.stars.iter().enumerate() {
        let lift = if i == 1 { 3. } else { 0. };
        let x = p.cx + (i as f32 - 1.) * 10.;
        pen.dot(x, p.cy - R - 9. - lift, 4.4, 4.4, mix(body, white, 0.2), *o);
    }
    // The line and the reflection are see-through tint, so they sit on any background.
    pen.rect(8., 73., 84., 2.4, 1.2, t0, 0.45);
    let light = 0.4 + 0.6 * p.glow;
    pen.dot(p.cx, 80., 26. * p.ref_w, 2.2, t0, p.ref_o * 0.5 * light);
    pen.dot(p.cx, 86., 14. * p.ref_w, 2.2, t0, p.ref_o * 0.3 * light);
}

/// What a mascot keeps between frames: when it was born and began its activity, and the
/// pose it was last drawn in, which a new activity blends from.
struct Memory {
    kind: Kind,
    born: Instant,
    since: Instant,
    from: Option<Pose>,
    last: Pose,
}

/// The pose to draw now; asks for the next frame while it moves, on the ticks every loop
/// of the window shares, so mascots and rings side by side cost one redraw per tick.
fn frame(kind: Kind, seed: f32, window: &mut Window, cx: &mut App) -> Pose {
    let now = cx.background_executor().now();
    let still = cx.reduce_motion();
    window.with_global_id("sundown".into(), |id, window| {
        window.with_element_state(id, |memory: Option<Memory>, window| {
            let mut memory = memory.unwrap_or(Memory {
                kind,
                born: now,
                since: now,
                from: None,
                last: BASE,
            });
            if memory.kind != kind {
                memory.from = Some(memory.last);
                memory.kind = kind;
                memory.since = now;
            }
            // Wrapped so the clock keeps its precision; the jump comes once an hour.
            let t = (now - memory.born).as_secs_f32() % 3600. + seed * 3.1;
            let (mut ts, mut k) = ((now - memory.since).as_secs_f32(), 1.);
            if still {
                ts = SETTLED;
            } else {
                k = clamp01(ts / BLEND);
            }
            // A shot takes longer than a hop, so the screenshot tool names the moment itself.
            #[cfg(feature = "screenshot")]
            if let Ok(Ok(ms)) = std::env::var("DUSK_MASCOT").map(|ms| ms.parse::<f32>()) {
                (ts, k) = (ms / 1000., 1.);
            }
            let mut p = pose(kind, if still { seed * 3.1 } else { t }, ts, seed);
            match memory.from {
                Some(from) if k < 1. => p = from.blend(&p, eio(k)),
                _ => memory.from = None,
            }
            memory.last = p;
            let moving = !kind.once() || ts < SETTLED || memory.from.is_some();
            if moving && !still {
                crate::ui::looping::ask_redraw(1, window, cx);
            }
            (p, memory)
        })
    })
}

/// The mascot in a box `unit` * 22 pixels wide; `color` is the agent's, worn by the sun.
/// The `id` keeps its memory between frames and seeds its blinks and glances, so mascots
/// side by side do not move in step.
pub fn render(
    id: impl Into<ElementId>,
    state: &AgentState,
    color: Hsla,
    unit: f32,
) -> impl IntoElement {
    let id = id.into();
    let mut hasher = DefaultHasher::new();
    id.hash(&mut hasher);
    let seed = (hasher.finish() % 4096) as f32 / 1024.;
    let kind = Kind::of(state);
    // The covers that shape the eyes must hide what is under them, so the sun is opaque.
    let tint = Rgba::from(Hsla { a: 1., ..color });
    div().id(id).flex_none().size(px(SIDE * unit)).child(
        canvas(
            move |_, window, cx| frame(kind, seed, window, cx),
            move |bounds, pose, window, _| paint(&pose, tint, bounds, window),
        )
        .size_full(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [Kind; 8] = [
        Kind::Thinking,
        Kind::Writing,
        Kind::Searching,
        Kind::Reading,
        Kind::Planning,
        Kind::Running,
        Kind::Done,
        Kind::Failed,
    ];

    // Done and failed stop asking for frames once settled, so they must not move after.
    #[::core::prelude::v1::test]
    fn poses_are_finite_and_done_and_failed_settle() {
        for kind in KINDS {
            for step in 0..400 {
                let t = step as f32 * 0.037;
                let p = pose(kind, t + 2.5, t, 1.3);
                let fields = [p.cx, p.cy, p.sx, p.sy, p.eye_o, p.gx, p.gy, p.glow];
                assert!(fields.iter().all(|v| v.is_finite()), "{kind:?} at {t}");
            }
            if kind.once() {
                let settled = pose(kind, 7., SETTLED, 1.3);
                assert_eq!(settled, pose(kind, 31., SETTLED + 9., 0.4), "{kind:?}");
            }
        }
    }
}
