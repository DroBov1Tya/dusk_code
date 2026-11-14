//! Night scenes: lanterns on a river and the northern lights over a cabin in the snow.

use crate::screensaver::paint::{
    Pt, Rgb, falling, fbm, glow, gradient, hash, mist, noise, ripples, smooth, stars,
};

pub fn lantern_river(p: Pt, t: f32) -> Rgb {
    const HORIZON: f32 = 0.5;
    if p.y < HORIZON {
        let mut color = gradient(
            &[
                (0.0, Rgb::hex(0x080a1e)),
                (0.35, Rgb::hex(0x1c1840)),
                (HORIZON, Rgb::hex(0x3a2440)),
            ],
            p.y,
        );
        color = color.add(Rgb::hex(0xffffff).scale(stars(p, t, 0.015) * 0.8));
        // Sky lanterns rising and swaying, smaller as they go.
        for i in 0..14 {
            let k = i as f32;
            let rise = ((t / (90.0 + hash(k, 1.0, 61) * 60.0)) + hash(k, 2.0, 61)).rem_euclid(1.0);
            let y = HORIZON - rise * 0.6;
            let x = (hash(k, 3.0, 61) - 0.5) * p.aspect + (t * 0.3 + k).sin() * 0.01;
            let size = 0.012 * (1.0 - rise * 0.6);
            color = color.add(Rgb::hex(0xff9a3a).scale(glow(p, x, y, size * 4.0) * 0.4));
            if ((p.x - x) / size).abs() < 0.7 && ((p.y - y) / size).abs() < 1.0 {
                color = Rgb::hex(0xffc46a).mix(Rgb::hex(0xff7a2a), (p.y - y) / size * 0.5 + 0.5);
            }
        }
        // Hills, then a town on the far bank with lit windows.
        let hills = HORIZON - 0.06 - fbm(p.x * 2.5, 9.0) * 0.1;
        if p.y > hills {
            color = Rgb::hex(0x141428);
        }
        // Houses on the far bank, each a block with a gable and a lit window or two.
        let house = (p.x * 22.0).floor();
        let within = (p.x * 22.0).rem_euclid(1.0);
        let height = 0.018 + hash(house, 1.0, 62) * 0.03;
        let gable = (0.5 - (within - 0.5).abs()) * 0.03;
        let roof = HORIZON - height - gable;
        if p.y > roof {
            let row = ((p.y - roof) * 140.0).floor();
            let window =
                (within - 0.3).abs() < 0.08 && hash(house, row, 63) > 0.55 && p.y > roof + 0.012;
            color = if window {
                Rgb::hex(0xffb050)
            } else {
                Rgb::hex(0x1a1628)
            };
        }
        return mist(p, t, HORIZON - 0.01, color, Rgb::hex(0x4b405c), 0.14);
    }
    let depth = (p.y - HORIZON) / (1.0 - HORIZON);
    let mut color = Rgb::hex(0x1a1630).mix(Rgb::hex(0x0c0c1c), depth);
    color = mist(p, t, HORIZON + 0.03, color, Rgb::hex(0x4b405c), 0.18);
    color = color.add(Rgb::hex(0x786985).scale(ripples(p, t, HORIZON) * 0.12));
    let ripple = noise(p.x * 20.0, p.y * 120.0 - t * 1.5);
    // Floating lanterns drift downstream, nearer ones larger, each with its reflection.
    for i in 0..26 {
        let k = i as f32;
        let row = hash(k, 1.0, 64);
        let y = HORIZON + 0.03 + row * row * 0.44;
        let near = (y - HORIZON) / (1.0 - HORIZON);
        let speed = 0.004 + near * 0.012;
        let span = p.aspect + 0.3;
        let x = ((hash(k, 2.0, 64) + t * speed / span).rem_euclid(1.0)) * span - span / 2.0;
        let size = 0.006 + near * 0.022;
        let bob = (t * 1.3 + k).sin() * size * 0.15;
        color = color.add(Rgb::hex(0xff8a2a).scale(glow(p, x, y + bob, size * 5.0) * 0.35));
        let (dx, dy) = ((p.x - x) / size, (p.y - y - bob) / size);
        if dx.abs() < 1.0 && dy > -0.9 && dy < 0.2 {
            color = Rgb::hex(0xffd88a).mix(Rgb::hex(0xff6a2a), dy + 0.9);
        }
        let below = dy > 0.3 && dy < 3.0 + near * 2.0 && dx.abs() < 0.8 * (1.0 - dy / 4.0);
        if below && ripple > 0.45 {
            color = color.mix(Rgb::hex(0xff9a3a), 0.5 * (1.0 - dy / 4.0));
        }
    }
    color
}

/// Where the cabin stands, as the middle of its foot, and its size against the picture.
const CABIN: (f32, f32, f32) = (-0.14, 0.9, 2.4);

pub fn aurora_cabin(p: Pt, t: f32) -> Rgb {
    let mut color = gradient(
        &[
            (0.0, Rgb::hex(0x050a18)),
            (0.5, Rgb::hex(0x0c1a2e)),
            (0.7, Rgb::hex(0x1a2a40)),
        ],
        p.y,
    );
    color = color.add(Rgb::hex(0xdfe8ff).scale(stars(p, t, 0.02) * 0.7));
    // Curtains: a bright lower hem along a slowly folding line, rays rising from it and
    // fading upwards from green into violet.
    for (index, (hem, reach)) in [(0.36, 0.26), (0.46, 0.2), (0.3, 0.16)]
        .into_iter()
        .enumerate()
    {
        let k = index as f32 * 2.7;
        let line = hem
            + (p.x * 1.7 + t * 0.04 + k).sin() * 0.05
            + (p.x * 4.3 - t * 0.07 + k).sin() * 0.02
            + (fbm(p.x * 1.2 + t * 0.015, k) - 0.5) * 0.12;
        let up = line - p.y;
        let rays = 0.35 + 0.65 * fbm(p.x * 26.0 + up * 3.0 + t * 0.15 + k, k + up * 1.5).powf(1.4);
        let body = if up >= 0.0 {
            (-up / reach).exp()
        } else {
            (up / 0.012).exp()
        };
        let pulse = 0.75 + 0.25 * (t * 0.4 + p.x * 2.0 + k).sin();
        let hue = Rgb::hex(0x52ffb0)
            .mix(Rgb::hex(0x30c0a0), smooth(0.0, 0.08, up))
            .mix(Rgb::hex(0x9a6aff), smooth(0.05, 0.25, up))
            .mix(Rgb::hex(0xff5ab0), smooth(-0.005, -0.03, up));
        let strength = [0.95, 0.6, 0.45][index];
        color = color.add(hue.scale(body * rays * pulse * strength));
    }
    // Snowy hills, pines on them and a cabin with a warm window.
    let hill = 0.7 - fbm(p.x * 1.8, 2.0) * 0.12;
    let pines = (0..3).any(|row| {
        let r = row as f32;
        let along = p.x * (9.0 - r * 2.0) + r * 0.37;
        let column = along.floor();
        if hash(column, r, 71) < 0.35 {
            return false;
        }
        let height = 0.08 + r * 0.04 + hash(column, r + 5.0, 71) * 0.05;
        let foot = hill + 0.01 + r * 0.06;
        let up = foot - p.y;
        let half = (along.rem_euclid(1.0) - 0.5).abs() / (9.0 - r * 2.0);
        up > 0.0 && up < height && half < (height - up) * 0.28
    });
    if p.y > hill {
        let lit = glow(p, CABIN.0, CABIN.1 - 0.04 * CABIN.2, 0.45);
        let drift = fbm(p.x * 6.0, p.y * 14.0);
        color = Rgb::hex(0x46587a)
            .mix(Rgb::hex(0xb8c8e8), smooth(0.3, 0.75, drift))
            .add(Rgb::hex(0x3cf0a0).scale(0.05))
            .add(Rgb::hex(0xffb060).scale(lit * 0.35));
    }
    if pines {
        color = Rgb::hex(0x0a1420);
    }
    color = mist(p, t, hill + 0.03, color, Rgb::hex(0x8293a6), 0.1);
    // A thin plume from the chimney bends with the same gentle wind as the snow.
    let (chimney_x, chimney_top) = (CABIN.0 + 0.035 * CABIN.2, CABIN.1 - 0.1 * CABIN.2);
    let up = chimney_top - p.y;
    if up > 0.0 && up < 0.3 {
        let x = chimney_x + up * 0.25 + (up * 12.0 - t * 0.3).sin() * up * 0.1;
        let plume = smooth(0.016 + up * 0.1, 0.0, (p.x - x).abs());
        color = color.mix(Rgb::hex(0x8b9cae), plume * smooth(0.3, 0.0, up) * 0.28);
    }
    if let Some(cabin) = cabin(p) {
        color = cabin;
    }
    match falling(p, t, 0.03, 0.0035, 0.3, 72) {
        Some(_) => Rgb::hex(0xf0f4ff),
        None => color,
    }
}

/// A log cabin under a snowy roof, with a chimney, a door and a lit window.
fn cabin(p: Pt) -> Option<Rgb> {
    let (x, ground, size) = CABIN;
    let (dx, dy) = ((p.x - x) / size, (ground - p.y) / size);
    let eave = (0.08 - dx.abs()).max(0.0) * 0.6;
    let wall = dx.abs() < 0.06 && (0.0..0.05).contains(&dy);
    let roof = dy >= 0.05 && dy < 0.05 + eave;
    let snow = roof && dy > 0.05 + eave - 0.014;
    let chimney = (dx - 0.035).abs() < 0.008 && dy > 0.06 && dy < 0.1;
    let window = (dx + 0.025).abs() < 0.014 && (dy - 0.027).abs() < 0.013;
    let door = (dx - 0.032).abs() < 0.011 && (0.0..0.034).contains(&dy);
    if window {
        let frame = (dx + 0.025).abs() < 0.002 || (dy - 0.027).abs() < 0.002;
        Some(if frame {
            Rgb::hex(0x4a2a14)
        } else {
            Rgb::hex(0xffc060)
        })
    } else if door {
        Some(Rgb::hex(0x1e120e))
    } else if snow {
        Some(Rgb::hex(0xf2f6ff))
    } else if wall {
        let gap = (dy / 0.0085).rem_euclid(1.0) < 0.14;
        Some(if gap {
            Rgb::hex(0x24160f)
        } else {
            Rgb::hex(0x6a4128)
        })
    } else if roof || chimney {
        Some(Rgb::hex(0x2a1a18))
    } else {
        None
    }
}
