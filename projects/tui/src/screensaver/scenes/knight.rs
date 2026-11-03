//! A knight resting by a fire at night: ruins on a hill, dead trees, fog, and the only
//! warm light the fire throws on his armour.

use crate::screensaver::paint::{Pt, Rgb, disk, fbm, glow, gradient, hash, noise, smooth};

const GROUND: f32 = 0.8;
const KNIGHT: (f32, f32) = (-0.26, 0.44);
const FIRE: (f32, f32) = (0.16, GROUND - 0.01);

pub fn knight_fire(p: Pt, t: f32) -> Rgb {
    let flicker = 0.85 + 0.15 * noise(t * 6.0, 1.0);
    let light = glow(p, FIRE.0, FIRE.1 - 0.06, 0.9) * flicker;
    let mut color = sky(p, t);
    color = ruins(p, color);
    color = dead_trees(p, color, light);
    if p.y > ground_line(p.x) {
        let earth = Rgb::hex(0x0b0a0c).mix(Rgb::hex(0x16120f), noise(p.x * 30.0, p.y * 40.0));
        color = earth.add(Rgb::hex(0x9a3a0c).scale(light.powf(1.6) * 0.9));
    }
    color = fog(p, t, color);
    color = smoke(p, t, color);
    if let Some(lit) = knight(p, light) {
        color = lit;
    }
    color = fire(p, t, color);
    color.add(Rgb::hex(0xff6a1a).scale(light.powi(3) * 0.18))
}

fn ground_line(x: f32) -> f32 {
    GROUND - (fbm(x * 3.0, 5.0) - 0.5) * 0.03
}

fn sky(p: Pt, t: f32) -> Rgb {
    let mut color = gradient(
        &[
            (0.0, Rgb::hex(0x06070d)),
            (0.3, Rgb::hex(0x161a28)),
            (0.58, Rgb::hex(0x3a4058)),
            (0.8, Rgb::hex(0x4a4254)),
        ],
        p.y,
    );
    // A pale moon behind slow clouds.
    let moon = (-0.55, 0.17);
    let clouds = smooth(0.4, 0.75, fbm(p.x * 2.0 + t * 0.006, p.y * 5.0));
    color = color.add(Rgb::hex(0x6a7088).scale(glow(p, moon.0, moon.1, 0.6) * 0.45));
    color = color.mix(
        Rgb::hex(0xb8bccb),
        disk(p, moon.0, moon.1, 0.05, 0.01) * (1.0 - clouds * 0.7),
    );
    color
        .mix(Rgb::hex(0x1a1c26), clouds * 0.6 * smooth(0.65, 0.2, p.y))
        .add(
            Rgb::hex(0x9aa0b8)
                .scale(crate::screensaver::paint::stars(p, t, 0.008) * 0.3 * (1.0 - clouds)),
        )
}

/// A broken keep on a far hill: towers with teeth, one fallen half away.
fn ruins(p: Pt, color: Rgb) -> Rgb {
    let hill = 0.62 - (-((p.x - 0.45) * 2.2).powi(2)).exp() * 0.1 - fbm(p.x * 2.0, 3.0) * 0.03;
    let mut top = hill;
    for (x, width, height, broken) in [
        (0.32, 0.035, 0.13, 0.0),
        (0.45, 0.05, 0.2, 0.0),
        (0.58, 0.035, 0.1, 0.4),
    ] {
        let dx = (p.x - x).abs();
        if dx < width {
            let teeth = ((p.x - x) * 140.0).rem_euclid(1.0) < 0.5;
            let ragged = broken * (noise(p.x * 60.0, 2.0) * 0.08);
            let roof = hill - height + ragged + if teeth { 0.0 } else { 0.008 };
            top = top.min(roof);
        }
    }
    let wall = (p.x - 0.45).abs() < 0.12 && p.y > hill - 0.03 - noise(p.x * 40.0, 4.0) * 0.03;
    if p.y > top || wall {
        let window = (p.x - 0.45).abs() < 0.006 && (p.y - (hill - 0.15)).abs() < 0.012;
        return if window {
            Rgb::hex(0x5a2a10)
        } else {
            Rgb::hex(0x1a1c26)
        };
    }
    color
}

/// Bare trees of forking branches, black against the sky, warmed by the fire's light.
fn dead_trees(p: Pt, color: Rgb, light: f32) -> Rgb {
    for (index, (x, height)) in [(-0.78, 0.5), (0.55, 0.42), (0.95, 0.55)]
        .into_iter()
        .enumerate()
    {
        if branch(
            p,
            x,
            ground_line(x),
            -std::f32::consts::FRAC_PI_2,
            height * 0.42,
            0.012,
            4,
            index as u32,
        ) {
            return Rgb::hex(0x07070a).add(Rgb::hex(0x5a2008).scale(light * 0.6));
        }
    }
    color
}

/// One branch and the two it forks into, `depth` times over.
#[allow(clippy::too_many_arguments)]
fn branch(
    p: Pt,
    x: f32,
    y: f32,
    angle: f32,
    length: f32,
    width: f32,
    depth: u32,
    seed: u32,
) -> bool {
    let (ex, ey) = (x + angle.cos() * length, y + angle.sin() * length);
    if segment(p, (x, y), (ex, ey)) < width {
        return true;
    }
    if depth == 0 {
        return false;
    }
    // Skip whole subtrees the point cannot be in.
    let reach = length * 2.2;
    if (p.x - ex).abs() > reach || (p.y - ey).abs() > reach {
        return false;
    }
    let k = depth as f32 + seed as f32 * 7.0;
    let spread = 0.35 + hash(k, 1.0, 81) * 0.35;
    let shrink = 0.62 + hash(k, 2.0, 81) * 0.15;
    branch(
        p,
        ex,
        ey,
        angle - spread,
        length * shrink,
        width * 0.68,
        depth - 1,
        seed * 2 + 1,
    ) || branch(
        p,
        ex,
        ey,
        angle + spread * 0.8,
        length * shrink * 0.9,
        width * 0.68,
        depth - 1,
        seed * 2 + 2,
    )
}

/// Distance from the point to a segment.
fn segment(p: Pt, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let k = (((p.x - a.0) * vx + (p.y - a.1) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    ((p.x - a.0 - vx * k).powi(2) + (p.y - a.1 - vy * k).powi(2)).sqrt()
}

fn fog(p: Pt, t: f32, color: Rgb) -> Rgb {
    let band = smooth(0.55, 0.72, p.y) * smooth(0.95, 0.75, p.y);
    let drift = fbm(p.x * 2.5 - t * 0.02, p.y * 9.0);
    color.mix(Rgb::hex(0x3a3c4a), band * smooth(0.35, 0.8, drift) * 0.6)
}

fn smoke(p: Pt, t: f32, color: Rgb) -> Rgb {
    let up = FIRE.1 - 0.1 - p.y;
    if up < 0.0 {
        return color;
    }
    let sway = (up * 4.0 - t * 0.3).sin() * 0.04 * up * 3.0;
    let width = 0.03 + up * 0.25;
    let core = smooth(width, 0.0, (p.x - FIRE.0 - sway - up * 0.15).abs());
    let wisps = fbm(p.x * 8.0, up * 6.0 - t * 0.4);
    color.mix(
        Rgb::hex(0x2a2a30),
        core * smooth(0.35, 0.7, wisps) * smooth(0.6, 0.1, up) * 0.6,
    )
}

fn fire(p: Pt, t: f32, color: Rgb) -> Rgb {
    let (dx, dy) = (p.x - FIRE.0, FIRE.1 - p.y);
    let mut color = color;
    // Stones ringing the fire, and the logs in it.
    let ring = ((dx / 0.1).powi(2) + ((dy + 0.004) / 0.018).powi(2)).sqrt();
    if (ring - 1.0).abs() < 0.22 && dy < 0.012 {
        let stone = noise(dx * 90.0, dy * 90.0);
        color = Rgb::hex(0x1a1816).add(Rgb::hex(0xc05a18).scale(0.35 + stone * 0.2));
    }
    let logs = dy > -0.004
        && dy < 0.018
        && dx.abs() < 0.08
        && ((dx * 1.4 + dy * 6.0).abs() < 0.012 || (dx * 1.4 - dy * 6.0).abs() < 0.012);
    if logs {
        color = Rgb::hex(0x2a1208).add(Rgb::hex(0xff7a20).scale(0.3 * noise(dx * 200.0, t * 3.0)));
    }
    if dy > 0.0 {
        // Tongues of flame, each curling and reaching its own height, hottest at the core.
        let mut heat: f32 = 0.0;
        for tongue in 0..6 {
            let k = tongue as f32;
            let base = (hash(k, 1.0, 84) - 0.5) * 0.09;
            let reach = 0.1 + hash(k, 2.0, 84) * 0.14 + noise(t * 2.5, k) * 0.06;
            let rise = dy / reach;
            if rise >= 1.0 {
                continue;
            }
            let sway = (dy * 22.0 - t * (5.0 + k) + k * 1.7).sin() * 0.014 * rise;
            let width = 0.026 * (1.0 - rise).powf(0.55) * (1.0 + (1.0 - rise) * 0.6);
            let across = (dx - base * (1.0 - rise * 0.6) - sway).abs() / width;
            heat = heat.max((1.0 - across) * (1.0 - rise * 0.5));
        }
        if heat > 0.0 {
            let flame = gradient(
                &[
                    (0.0, Rgb::hex(0x7a1404)),
                    (0.35, Rgb::hex(0xd8400a)),
                    (0.7, Rgb::hex(0xff9a30)),
                    (1.0, Rgb::hex(0xfff2b8)),
                ],
                heat,
            );
            color = color.mix(flame, smooth(0.0, 0.2, heat));
        }
        // Embers climbing in the warm air.
        let column = (dx * 70.0).floor();
        let climb = dy * 7.0 + t * (0.5 + hash(column, 1.0, 82) * 0.8);
        if hash(column, climb.floor(), 83) > 0.94
            && climb.rem_euclid(1.0) < 0.12
            && dy < 0.5
            && dx.abs() < 0.2
        {
            color = Rgb::hex(0xffa040).scale(1.0 - dy * 1.6);
        }
    }
    color
}

/// The knight on a rock, side on, turned to the fire on his right: the colour where the
/// point is on him, lit along the edges that face the fire.
fn knight(p: Pt, light: f32) -> Option<Rgb> {
    let part = figure(p)?;
    // An edge facing the fire is where the figure ends a little way towards it.
    let toward = {
        let (dx, dy) = (FIRE.0 - p.x, FIRE.1 - 0.05 - p.y);
        let length = (dx * dx + dy * dy).sqrt().max(1e-3);
        (dx / length * 0.012, dy / length * 0.012)
    };
    let rim = figure(p.at(p.x + toward.0, p.y + toward.1)).is_none();
    let base = match part {
        Part::Steel => Rgb::hex(0x0e1016),
        Part::Cloak => Rgb::hex(0x12080a),
        Part::Leather => Rgb::hex(0x120c0a),
        Part::Stone => Rgb::hex(0x0e0e10),
        Part::Visor => Rgb::hex(0x020203),
    };
    let sheen = match part {
        Part::Steel => 1.0,
        Part::Stone => 0.35,
        Part::Visor => 0.0,
        _ => 0.5,
    };
    let warm = Rgb::hex(0xff7a2a).scale(light.powf(1.2) * sheen * 0.7);
    let edge = Rgb::hex(0xffb060).scale(if rim { light * sheen * 1.3 } else { 0.0 });
    Some(base.add(warm).add(edge))
}

#[derive(Clone, Copy)]
enum Part {
    Steel,
    Cloak,
    Leather,
    Stone,
    Visor,
}

/// Which part of the knight, his sword, shield or seat is at the point.
fn figure(p: Pt) -> Option<Part> {
    let size = KNIGHT.1;
    let (u, v) = ((p.x - KNIGHT.0) / size, (GROUND - p.y) / size);
    if !(-0.6..0.6).contains(&u) || !(-0.05..1.0).contains(&v) {
        return None;
    }
    let near = |a: (f32, f32), b: (f32, f32), width: f32| {
        let (vx, vy) = (b.0 - a.0, b.1 - a.1);
        let k = (((u - a.0) * vx + (v - a.1) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
        ((u - a.0 - vx * k).powi(2) + (v - a.1 - vy * k).powi(2)).sqrt() < width
    };
    let round = |cx: f32, cy: f32, rx: f32, ry: f32| {
        ((u - cx) / rx).powi(2) + ((v - cy) / ry).powi(2) < 1.0
    };
    // Head and helm, with a dark slit towards the fire and a short crest.
    if round(0.06, 0.8, 0.07, 0.08) {
        let slit = (v - 0.8).abs() < 0.012 && u > 0.07;
        return Some(if slit { Part::Visor } else { Part::Steel });
    }
    if near((0.02, 0.88), (-0.06, 0.93), 0.018) {
        return Some(Part::Cloak);
    }
    // Arms: the near one rests its forearm on the knee, hand to the fire.
    if near((0.03, 0.68), (0.16, 0.5), 0.045) || near((0.16, 0.5), (0.31, 0.53), 0.038) {
        return Some(Part::Steel);
    }
    if round(0.33, 0.53, 0.035, 0.03) {
        return Some(Part::Leather);
    }
    // Torso leaning forward, a belt, then the legs: thigh along the rock, shin down.
    if near((0.0, 0.36), (0.04, 0.68), 0.09) {
        let belt = (v - 0.4).abs() < 0.018;
        return Some(if belt { Part::Leather } else { Part::Steel });
    }
    if near((0.02, 0.33), (0.27, 0.37), 0.06) || near((0.28, 0.36), (0.33, 0.04), 0.05) {
        return Some(Part::Steel);
    }
    if round(0.36, 0.03, 0.07, 0.035) {
        return Some(Part::Leather);
    }
    // The cloak falls from the shoulders behind him to the rock, its hem torn.
    let hem = 0.12 + (u * 40.0).sin().abs() * 0.04 + noise(u * 30.0, 3.0) * 0.04;
    let back = -0.05 - (0.72 - v) * 0.32;
    if v > hem && v < 0.72 && u > back && u < 0.04 {
        return Some(Part::Cloak);
    }
    // The sword driven into the ground before him, a shield against the rock.
    if (u + 0.42).abs() < 0.012 && v < 0.5 {
        return Some(Part::Steel);
    }
    if (v - 0.5).abs() < 0.014 && (u + 0.42).abs() < 0.075 {
        return Some(Part::Steel);
    }
    if (u + 0.42).abs() < 0.014 && v > 0.5 && v < 0.62 {
        return Some(Part::Leather);
    }
    if round(-0.42, 0.64, 0.022, 0.022) {
        return Some(Part::Steel);
    }
    let shield = ((u + 0.26) / 0.1).powi(2) + ((v - 0.2) / 0.16).powi(2) < 1.0 && v > 0.05;
    if shield {
        return Some(Part::Steel);
    }
    if round(0.0, 0.14, 0.24, 0.16) && v > 0.0 {
        return Some(Part::Stone);
    }
    None
}
