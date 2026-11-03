//! Autumn scenes: a cyclist on a country road, a pagoda among maples and a forest with
//! light falling through it.

use crate::screensaver::paint::{Pt, Rgb, falling, fbm, glow, gradient, hash, noise, smooth};

/// Autumn leaf colours, by a 0..1 pick.
fn leaf(pick: f32) -> Rgb {
    gradient(
        &[
            (0.0, Rgb::hex(0xc8321e)),
            (0.4, Rgb::hex(0xe8742a)),
            (0.75, Rgb::hex(0xf2b33a)),
            (1.0, Rgb::hex(0xa8461e)),
        ],
        pick,
    )
}

/// Leaves drifting down over everything.
fn leaves(p: Pt, t: f32, color: Rgb) -> Rgb {
    match falling(p, t, 0.035, 0.006, 0.25, 41) {
        Some(pick) => leaf(pick),
        None => color,
    }
}

/// A round crown of leaves around cx, cy, with a ragged edge: its colour where it is.
fn crown(p: Pt, cx: f32, cy: f32, radius: f32, seed: f32) -> Option<Rgb> {
    let (dx, dy) = (p.x - cx, (p.y - cy) * 1.15);
    let distance = (dx * dx + dy * dy).sqrt() / radius;
    if distance > 1.2 {
        return None;
    }
    let ragged = fbm(p.x / radius * 3.0 + seed, p.y / radius * 3.0);
    if distance > 0.75 + ragged * 0.45 {
        return None;
    }
    let pick = noise(p.x / radius * 6.0 + seed, p.y / radius * 6.0);
    // Lit from the upper left, darker inside and underneath.
    let light = smooth(0.9, -0.6, (dx + dy) / radius) * 0.5 + ragged * 0.6;
    Some(leaf(pick).scale(0.45 + light * 0.75))
}

pub fn country_road(p: Pt, t: f32) -> Rgb {
    const HORIZON: f32 = 0.46;
    let mut color = gradient(
        &[
            (0.0, Rgb::hex(0x6a9ad0)),
            (0.3, Rgb::hex(0xc8d0d0)),
            (HORIZON, Rgb::hex(0xf6d2a0)),
        ],
        p.y,
    );
    let hills = HORIZON - 0.03 - fbm(p.x * 3.0, 3.0) * 0.06;
    if p.y > hills {
        color = Rgb::hex(0xb07a5a).mix(Rgb::hex(0xd8a070), noise(p.x * 20.0, p.y * 20.0));
    }
    if p.y > HORIZON {
        // Fields, then the road narrowing to the horizon with its centre line passing.
        let depth = p.y - HORIZON;
        let z = 0.12 / depth;
        color = Rgb::hex(0xa8642a).mix(Rgb::hex(0xd89040), noise(p.x * z * 3.0, z * 2.0));
        let half = 0.09 * depth / 0.12 * 1.6;
        if p.x.abs() < half {
            color = Rgb::hex(0x4a4248).mix(Rgb::hex(0x6a6066), smooth(0.0, 0.5, depth));
            let dash = (z * 0.8 + t * 0.9).rem_euclid(1.0) < 0.45;
            if dash && p.x.abs() < half * 0.03 {
                color = Rgb::hex(0xf0e0b8);
            }
        } else if p.x.abs() < half * 1.08 {
            color = Rgb::hex(0x7a5a3a);
        }
    }
    // Trees on both sides come nearer and pass: far ones first, near ones over them.
    const TREES: usize = 16;
    let mut order: Vec<f32> = (0..TREES)
        .map(|i| (i as f32 * 0.55 - t * 0.12).rem_euclid(TREES as f32 * 0.55) + 0.35)
        .collect();
    order.sort_by(|a, b| b.total_cmp(a));
    for (i, z) in order.into_iter().enumerate() {
        let ground = HORIZON + 0.12 / z;
        let scale = 1.0 / z;
        for side in [-1.0f32, 1.0] {
            let x = side * (0.2 + 0.06 * hash(i as f32, side, 44)) * scale * 1.6;
            let trunk =
                (p.x - x).abs() < 0.012 * scale && p.y < ground && p.y > ground - 0.22 * scale;
            if trunk {
                color = Rgb::hex(0x3a2418);
            }
            let fade = smooth(9.0, 4.0, z);
            if let Some(leaves) = crown(p, x, ground - 0.26 * scale, 0.13 * scale, i as f32 + side)
            {
                color = leaves.mix(Rgb::hex(0xf2d0a0), (1.0 - fade) * 0.5);
            }
        }
    }
    let bob = (t * 6.0).sin().abs() * 0.004;
    if let Some(rider) = cyclist(p, 0.03, 0.86 - bob, 0.11, t) {
        color = rider;
    }
    leaves(p, t, color)
}

/// A cyclist riding right, wheels on `ground`, `size` tall.
fn cyclist(p: Pt, x: f32, ground: f32, size: f32, t: f32) -> Option<Rgb> {
    let (dx, dy) = ((p.x - x) / size, (ground - p.y) / size);
    let wheel = |cx: f32| {
        let r = ((dx - cx).powi(2) + (dy - 0.22).powi(2)).sqrt();
        (r - 0.2).abs() < 0.03 || r < 0.03
    };
    let line = |ax: f32, ay: f32, bx: f32, by: f32, w: f32| {
        let (vx, vy) = (bx - ax, by - ay);
        let k = (((dx - ax) * vx + (dy - ay) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
        ((dx - ax - vx * k).powi(2) + (dy - ay - vy * k).powi(2)).sqrt() < w
    };
    let frame = line(-0.3, 0.22, 0.0, 0.22, 0.025)
        || line(0.0, 0.22, 0.12, 0.5, 0.025)
        || line(-0.3, 0.22, -0.08, 0.5, 0.025)
        || line(0.3, 0.22, 0.2, 0.55, 0.025);
    let pedal = (t * 6.0).sin() * 0.1;
    let body = line(-0.05, 0.55, 0.12, 0.85, 0.06)
        || line(-0.05, 0.55, pedal, 0.22 + pedal.abs(), 0.04)
        || line(0.12, 0.8, 0.22, 0.58, 0.035);
    let head = ((dx - 0.17).powi(2) + (dy - 0.95).powi(2)).sqrt() < 0.08;
    let scarf = line(0.08, 0.85, -0.15 - (t * 4.0).sin() * 0.03, 0.82, 0.03);
    if scarf {
        Some(Rgb::hex(0xd8382a))
    } else if wheel(-0.3) || wheel(0.3) || frame || body || head {
        Some(Rgb::hex(0x241a1a))
    } else {
        None
    }
}

pub fn pagoda(p: Pt, t: f32) -> Rgb {
    let mut color = gradient(
        &[
            (0.0, Rgb::hex(0xa8b8d0)),
            (0.45, Rgb::hex(0xf2d8c0)),
            (1.0, Rgb::hex(0xe0a888)),
        ],
        p.y,
    );
    // Far hills in mist, then the pagoda, then maples framing it on both sides.
    for (index, (base, shade)) in [(0.5, 0.25), (0.62, 0.45)].into_iter().enumerate() {
        let hills = base - fbm(p.x * 2.0, 5.0 + index as f32) * 0.2;
        if p.y > hills {
            color = color.mix(Rgb::hex(0x8a6a80), shade);
        }
    }
    if let Some(tower) = tower(p) {
        color = tower;
    }
    // Maples lean in from both sides, crowns overlapping into a frame.
    for side in [-1.0f32, 1.0] {
        let edge = side * p.aspect / 2.0;
        let trunk = (p.x - (edge - side * (0.12 + p.y * 0.12))).abs() < 0.018 + p.y * 0.02;
        if trunk && p.y > 0.35 {
            color = Rgb::hex(0x3a1e1a);
        }
        for k in 0..6 {
            let k = k as f32;
            let cx = edge - side * (0.05 + hash(k, side, 45) * 0.32);
            let cy = 0.05 + k * 0.08 + hash(k, side + 3.0, 45) * 0.05;
            if let Some(maple) = crown(
                p,
                cx,
                cy,
                0.12 + hash(k, side + 7.0, 45) * 0.06,
                k + side * 9.0,
            ) {
                color = maple.mix(Rgb::hex(0xe8301e), 0.25);
            }
        }
    }
    if p.y > 0.86 {
        color = Rgb::hex(0x5a3a30).mix(Rgb::hex(0xc8502a), noise(p.x * 30.0, p.y * 30.0) * 0.6);
    }
    let mist = smooth(0.55, 0.75, fbm(p.x * 3.0 + t * 0.01, p.y * 8.0)) * smooth(0.6, 0.85, p.y);
    leaves(p, t, color.mix(Rgb::hex(0xf2e4dc), mist * 0.5))
}

/// Five tiers with upturned eaves, narrowing upwards, under a spire.
fn tower(p: Pt) -> Option<Rgb> {
    let dark = Rgb::hex(0x3a1e22);
    let roof = Rgb::hex(0x2a161c);
    if p.x.abs() < 0.004 && p.y > 0.08 && p.y < 0.2 {
        return Some(roof);
    }
    for tier in 0..5 {
        let k = tier as f32;
        let bottom = 0.86 - k * 0.13;
        let half = 0.12 - k * 0.018;
        let eave = bottom - 0.1;
        let lift = (p.x.abs() / (half + 0.06)).powi(4) * 0.03;
        let roof_line = eave - 0.02 + lift;
        if p.y > roof_line && p.y < eave + 0.012 + lift && p.x.abs() < half + 0.06 {
            return Some(roof);
        }
        if p.y >= eave && p.y < bottom && p.x.abs() < half {
            let window = p.x.abs() < half * 0.3 && p.y > eave + 0.03;
            return Some(if window { Rgb::hex(0xf2a04a) } else { dark });
        }
    }
    None
}

pub fn forest(p: Pt, t: f32) -> Rgb {
    let sun = (0.2, -0.15);
    let haze = Rgb::hex(0xf8e0b0);
    let mut color = haze.mix(Rgb::hex(0xe0a868), smooth(0.3, 1.0, p.y));
    // Rows of trees, far ones pale in the haze and small, near ones dark and large: a
    // tapering trunk under a crown of several clumps.
    let rows = [
        (0.75, 0.55, 0.012, 0.0),
        (0.55, 0.68, 0.02, 1.3),
        (0.3, 0.8, 0.032, 2.9),
        (0.0, 0.95, 0.05, 4.1),
    ];
    for (index, (mist, foot, width, seed)) in rows.into_iter().enumerate() {
        let spacing = width * 9.0;
        let along = p.x / spacing + seed;
        let column = along.floor();
        for slot in [column - 1.0, column, column + 1.0] {
            if hash(slot, index as f32, 51) < 0.15 {
                continue;
            }
            let x = (slot + 0.25 + hash(slot, 2.0, 51) * 0.5 - seed) * spacing;
            let height = (0.35 + hash(slot, 3.0, 51) * 0.25) * (foot - 0.2);
            let top = foot - height * 2.2;
            let wide = width * (0.5 + 0.5 * smooth(top, foot, p.y));
            let dx = p.x - x;
            if dx.abs() < wide && p.y < foot && p.y > top {
                let bark = Rgb::hex(0x3a2216).scale(0.75 + noise(dx * 400.0, p.y * 30.0) * 0.5);
                let shaded = bark.mix(Rgb::hex(0xa87048), smooth(wide, -wide, dx) * 0.45);
                color = shaded.mix(haze, mist);
            }
            for clump in 0..4 {
                let k = clump as f32;
                let cx = x + (hash(slot, k, 52) - 0.5) * spacing * 0.9;
                let cy = top + (hash(slot, k + 4.0, 52) - 0.3) * height * 0.5;
                let radius = spacing * (0.35 + hash(slot, k + 8.0, 52) * 0.25);
                if let Some(leaves) = crown(p, cx, cy, radius, slot * 3.0 + k) {
                    color = leaves.mix(haze, mist * 0.85);
                }
            }
        }
    }
    // Light through the leaves in slanting rays, and the floor of fallen leaves.
    let ray = smooth(0.6, 0.9, noise((p.x - p.y * 0.45) * 9.0 + t * 0.02, 1.0));
    color = color.add(Rgb::hex(0xffe8b0).scale(ray * 0.28 * glow(p, sun.0, sun.1, 1.8)));
    let floor = 0.93 + (fbm(p.x * 4.0, 8.0) - 0.5) * 0.03;
    if p.y > floor {
        color = leaf(noise(p.x * 50.0, p.y * 50.0)).scale(0.6 + (p.y - floor) * 3.0);
    }
    leaves(p, t, color)
}
