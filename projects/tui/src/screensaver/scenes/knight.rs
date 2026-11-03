//! A weary knight at a small fire: bare autumn woods, the first snow, and cold
//! moonlight on worn plate. The fire is the only warm light in the clearing.

use crate::screensaver::paint::{Pt, Rgb, disk, falling, fbm, glow, gradient, hash, noise, smooth};

const GROUND: f32 = 0.8;
const KNIGHT: (f32, f32) = (-0.23, 0.62);
const FIRE: (f32, f32) = (0.16, GROUND - 0.01);

pub fn knight_fire(p: Pt, t: f32) -> Rgb {
    let flicker = 0.85 + 0.15 * noise(t * 6.0, 1.0);
    let light = glow(p, FIRE.0, FIRE.1 - 0.06, 0.7) * flicker;
    let mut color = sky(p, t);
    color = ruins(p, color);
    color = forest(p, color);
    color = dead_trees(p, color, light);
    if p.y > ground_line(p.x) {
        let earth = Rgb::hex(0x0b0a0c).mix(Rgb::hex(0x16120f), noise(p.x * 30.0, p.y * 40.0));
        color = earth.add(Rgb::hex(0x9a3a0c).scale(light.powf(1.6) * 0.65));
        // The first frost lies in broken patches, with dark earth and copper leaves
        // still showing through. No continuous winter snow blanket.
        let frost = fbm(p.x * 23.0, p.y * 51.0);
        if frost > 0.64 && noise(p.x * 75.0, p.y * 100.0) > 0.4 {
            color = Rgb::hex(0x465460).add(Rgb::hex(0xbc7136).scale(light * 0.45));
        } else if noise(p.x * 130.0, p.y * 170.0) > 0.77 {
            color = Rgb::hex(0x49321e).add(Rgb::hex(0x9a4716).scale(light * 0.5));
        }
    }
    color = fog(p, t, color);
    color = smoke(p, t, color);
    if let Some(lit) = knight(p, light) {
        color = lit;
    }
    color = fire(p, t, color);
    color = color.add(Rgb::hex(0xff6a1a).scale(light.powi(3) * 0.1));
    // A few slow flakes cross the clearing; most of the air stays empty.
    if falling(p, t, 0.025, 0.003, 0.012, 91).is_some() {
        color = color.mix(Rgb::hex(0x9aaebc), 0.72);
    }
    color
}

fn ground_line(x: f32) -> f32 {
    GROUND - (fbm(x * 3.0, 5.0) - 0.5) * 0.03
}

fn sky(p: Pt, t: f32) -> Rgb {
    let mut color = gradient(
        &[
            (0.0, Rgb::hex(0x050a10)),
            (0.3, Rgb::hex(0x0d1b27)),
            (0.58, Rgb::hex(0x243744)),
            (0.8, Rgb::hex(0x303d45)),
        ],
        p.y,
    );
    // A pale moon behind slow clouds.
    let moon = (0.48, 0.16);
    let clouds = smooth(0.4, 0.75, fbm(p.x * 2.0 + t * 0.006, p.y * 5.0));
    color = color.add(Rgb::hex(0x6a7088).scale(glow(p, moon.0, moon.1, 0.6) * 0.45));
    color = color.mix(
        Rgb::hex(0xb8bccb),
        disk(p, moon.0, moon.1, 0.026, 0.012) * (1.0 - clouds * 0.7),
    );
    color
        .mix(Rgb::hex(0x1a1c26), clouds * 0.6 * smooth(0.65, 0.2, p.y))
        .add(
            Rgb::hex(0x9aa0b8)
                .scale(crate::screensaver::paint::stars(p, t, 0.002) * 0.18 * (1.0 - clouds)),
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

/// Thin distant trunks fade into the mist instead of forming a flat skyline.
fn forest(p: Pt, color: Rgb) -> Rgb {
    let mut color = color;
    for layer in 0..2 {
        let spacing = 0.13 + layer as f32 * 0.12;
        let column = (p.x / spacing).floor();
        let x = (column + 0.2 + hash(column, layer as f32, 92) * 0.6) * spacing;
        let base = 0.68 + layer as f32 * 0.08;
        let bend = (p.y * 6.0 + column).sin() * 0.008;
        let width = 0.006 + layer as f32 * 0.007;
        let trunk = (p.x - x - bend).abs() < width * (0.5 + p.y);
        let limb = segment(p, (x, base - 0.32), (x + 0.075, base - 0.45)) < width * 0.35;
        if p.y < base && (trunk || limb) {
            color = if layer == 0 {
                Rgb::hex(0x162630)
            } else {
                Rgb::hex(0x0c1920)
            };
        }
    }
    color
}

/// Bare trees of forking branches, black against the sky, warmed by the fire's light.
fn dead_trees(p: Pt, color: Rgb, light: f32) -> Rgb {
    for (index, (x, height)) in [(-0.66, 0.95), (0.68, 1.05), (1.05, 0.85)]
        .into_iter()
        .enumerate()
    {
        if branch(
            p,
            x,
            ground_line(x),
            -std::f32::consts::FRAC_PI_2,
            height * 0.55,
            0.025,
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
    color.mix(Rgb::hex(0x394955), band * smooth(0.35, 0.8, drift) * 0.3)
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
            let reach = 0.08 + hash(k, 2.0, 84) * 0.09 + noise(t * 2.5, k) * 0.035;
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
        Part::Steel => Rgb::hex(0x263541),
        Part::Polished => Rgb::hex(0x435766),
        Part::Cloak => Rgb::hex(0x15151c),
        Part::Leather => Rgb::hex(0x120c0a),
        Part::Stone => Rgb::hex(0x0e0e10),
        Part::Visor => Rgb::hex(0x020203),
    };
    let sheen = match part {
        Part::Steel => 0.8,
        Part::Polished => 1.0,
        Part::Stone => 0.35,
        Part::Visor => 0.0,
        _ => 0.5,
    };
    let warm = Rgb::hex(0xff7a2a).scale(light.powf(1.2) * sheen * 0.43);
    let edge = Rgb::hex(0xffb060).scale(if rim { light * sheen * 1.3 } else { 0.0 });
    Some(base.add(warm).add(edge))
}

#[derive(Clone, Copy)]
enum Part {
    Steel,
    Polished,
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
    // A bowed closed helm, with a narrow visor and a raised brow. Its cold
    // upper plane stays readable when the fire flickers low.
    if round(0.08, 0.84, 0.105, 0.115) {
        if (v - (0.84 - u * 0.22)).abs() < 0.014 && u > 0.025 {
            return Some(Part::Visor);
        }
        if v < 0.8
            && u > 0.06
            && ((u * 60.0).floor() as i32 % 2 == 0)
            && ((v * 60.0).floor() as i32 % 2 == 0)
        {
            return Some(Part::Visor);
        }
        return Some(if v > 0.88 || (u + 0.005).abs() < 0.012 {
            Part::Polished
        } else {
            Part::Steel
        });
    }
    // Gauntlets rest together on the pommel of a grounded sword.
    if round(0.25, 0.52, 0.065, 0.035) || round(0.19, 0.55, 0.06, 0.03) {
        return Some(Part::Polished);
    }
    if (u - 0.24).abs() < 0.017 && v > 0.015 && v < 0.49 {
        return Some(if u < 0.24 {
            Part::Polished
        } else {
            Part::Steel
        });
    }
    if (v - 0.46).abs() < 0.014 && (u - 0.24).abs() < 0.09 {
        return Some(Part::Steel);
    }
    // Rounded shoulder plates and articulated forearms, ahead of the breastplate.
    if round(-0.005, 0.68, 0.105, 0.075) {
        return Some(if v > 0.705 {
            Part::Polished
        } else {
            Part::Steel
        });
    }
    if near((0.0, 0.64), (0.06, 0.51), 0.05)
        || near((0.06, 0.51), (0.21, 0.54), 0.036)
        || near((0.17, 0.65), (0.25, 0.55), 0.033)
    {
        return Some(if (v - 0.58).abs() < 0.008 || (v - 0.5).abs() < 0.008 {
            Part::Leather
        } else {
            Part::Steel
        });
    }
    // Two bent legs make the seated pose clear even at terminal resolution.
    if round(0.3, 0.34, 0.07, 0.075) || round(0.1, 0.32, 0.06, 0.065) {
        return Some(Part::Polished);
    }
    if near((0.02, 0.36), (0.29, 0.36), 0.065)
        || near((0.3, 0.29), (0.34, 0.055), 0.038)
        || near((0.08, 0.28), (0.09, 0.045), 0.035)
    {
        return Some(Part::Steel);
    }
    if near((0.34, 0.04), (0.43, 0.025), 0.032) || near((0.09, 0.035), (0.15, 0.015), 0.03) {
        return Some(Part::Leather);
    }
    // Leaning torso with a dark belt and an overlapping lower plate.
    if near((-0.04, 0.39), (0.07, 0.68), 0.1) {
        return Some(if (v - 0.42).abs() < 0.018 {
            Part::Leather
        } else if u > 0.065 && v > 0.47 {
            Part::Polished
        } else {
            Part::Steel
        });
    }
    // Heavy charcoal cloak, ragged at the hem, falling behind the seat.
    let hem = 0.035 + (u * 40.0).sin().abs() * 0.025;
    let back = -0.12 - (0.7 - v) * 0.25;
    if v > hem && v < 0.74 && u > back && u < 0.06 {
        return Some(Part::Cloak);
    }
    if round(-0.04, 0.19, 0.23, 0.19) && v > 0.0 {
        return Some(Part::Stone);
    }
    None
}
