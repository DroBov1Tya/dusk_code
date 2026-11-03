//! Scenes over water: a mountain lake and a river at dawn, the sea and a neon horizon at
//! sunset.

use crate::screensaver::paint::{
    Pt, Rgb, bird, crossing, disk, fbm, glow, gradient, hash, noise, smooth, stars,
};

/// What lies under a horizon at `horizon` mirrors what is above it, rippled and tinted.
fn mirror(p: Pt, t: f32, horizon: f32, water: Rgb, above: impl Fn(Pt) -> Rgb) -> Rgb {
    let depth = (p.y - horizon) / (1.0 - horizon);
    let ripple = (p.y * 220.0 - t * 2.5).sin() * 0.004 * (0.3 + depth)
        + (noise(p.x * 18.0, p.y * 90.0 - t * 0.8) - 0.5) * 0.02 * depth;
    let mirrored = p.at(p.x + ripple, horizon - (p.y - horizon) * 0.9);
    above(mirrored).mix(water, 0.3 + depth * 0.4)
}

pub fn fuji_lake(p: Pt, t: f32) -> Rgb {
    const HORIZON: f32 = 0.64;
    const SUMMIT: f32 = 0.2;
    let above = |p: Pt| {
        let sky = gradient(
            &[
                (0.0, Rgb::hex(0x1e2350)),
                (0.2, Rgb::hex(0x5a4a8a)),
                (0.4, Rgb::hex(0xd47c9e)),
                (0.55, Rgb::hex(0xf7a58e)),
                (HORIZON, Rgb::hex(0xffd29a)),
            ],
            p.y,
        );
        let sun = glow(p, -0.55, HORIZON, 0.7);
        let mut color = sky.add(Rgb::hex(0xffb080).scale(sun * 0.35));
        let streak = smooth(0.55, 0.75, fbm(p.x * 1.5 - t * 0.008, p.y * 14.0));
        let lit_cloud = Rgb::hex(0xffd0d8).mix(Rgb::hex(0x9a6a9a), smooth(0.25, 0.5, p.y));
        color = color.mix(
            lit_cloud,
            streak * smooth(0.04, 0.2, p.y) * smooth(0.5, 0.3, p.y),
        );
        // A concave cone: steep under the crater, spreading wide at its foot.
        let reach = (p.x.abs() - 0.035).max(0.0);
        let ridge =
            SUMMIT + 0.48 * (1.0 - (-reach / 0.34).exp()) + (noise(p.x * 50.0, 1.0) - 0.5) * 0.006;
        if p.y > ridge {
            // Ridges and gullies run down from the summit like spokes.
            let spoke = p.x / (p.y - SUMMIT + 0.05);
            let gully = fbm(spoke * 5.0, 3.0);
            let lit = smooth(0.25, -0.35, p.x + (gully - 0.5) * 0.2);
            let rock = Rgb::hex(0x2e2a52)
                .mix(Rgb::hex(0x6a4f82), lit * 0.8)
                .scale(0.8 + gully * 0.4);
            let snowline = SUMMIT + 0.1 + gully * 0.16 + (fbm(p.x * 20.0, p.y * 8.0) - 0.5) * 0.03;
            let snow = Rgb::hex(0x9c90c8)
                .mix(Rgb::hex(0xfff0f4), lit)
                .scale(0.85 + gully * 0.2);
            color = rock.mix(snow, smooth(snowline + 0.01, snowline - 0.01, p.y));
            let foot = smooth(HORIZON - 0.15, HORIZON, p.y);
            color = color.mix(Rgb::hex(0x3a3058), foot * 0.6);
        }
        let hills = HORIZON - 0.035 + (fbm(p.x * 5.0, 7.0) - 0.5) * 0.05;
        if p.y > hills {
            let trees = noise(p.x * 140.0, 2.0) * 0.012;
            color = if p.y > hills - trees + 0.006 {
                Rgb::hex(0x1e1a34)
            } else {
                Rgb::hex(0x2a2444)
            };
        }
        for i in 0..5 {
            let i = i as f32;
            let x = crossing(p, t, 80.0, 0.35 + i * 0.03);
            let y = 0.3 + i * 0.03 + (t * 0.3 + i).sin() * 0.008;
            if bird(p, x + i * 0.05, y, 0.014, t + i) {
                color = Rgb::hex(0x2a2140);
            }
        }
        color
    };
    if p.y < HORIZON {
        return above(p);
    }
    let mut color = mirror(p, t, HORIZON, Rgb::hex(0x2c3460), above);
    let glint = smooth(0.7, 0.9, noise(p.x * 40.0, p.y * 260.0 - t * 2.0));
    color = color.add(Rgb::hex(0xffc0a0).scale(glint * 0.15));
    // A small boat poled across the lake.
    let boat = crossing(p, t, 90.0, 0.45);
    let (dx, dy) = (p.x - boat, p.y - 0.8);
    let hull = dy > -0.006 && dy < 0.008 && dx.abs() < 0.045 - dy * 2.0;
    let person = (dx - 0.01).abs() < 0.005 && dy > -0.04 && dy < 0.0;
    let pole = (dx + (dy + 0.02) * 0.6 + 0.02).abs() < 0.0025 && dy > -0.06 && dy < 0.04;
    if hull || person || pole {
        color = Rgb::hex(0x16132a);
    }
    color
}

pub fn river_dawn(p: Pt, t: f32) -> Rgb {
    const HORIZON: f32 = 0.55;
    let sun = (0.18, 0.47);
    let above = |p: Pt| {
        let mut color = gradient(
            &[
                (0.0, Rgb::hex(0x3a2f5a)),
                (0.25, Rgb::hex(0xb15c6e)),
                (0.45, Rgb::hex(0xf2a65a)),
                (HORIZON, Rgb::hex(0xffd88a)),
            ],
            p.y,
        );
        color = color.add(Rgb::hex(0xffc36a).scale(glow(p, sun.0, sun.1, 0.45) * 0.6));
        color = color.mix(Rgb::hex(0xfff1c4), disk(p, sun.0, sun.1, 0.06, 0.01));
        let haze = smooth(0.6, 0.75, fbm(p.x * 2.5 + t * 0.008, p.y * 10.0));
        color = color.mix(Rgb::hex(0xf7c08a), haze * 0.5 * smooth(0.1, 0.4, p.y));
        // The far bank: steps rising to temples with spires.
        let along = p.x * 8.0;
        let block = along.floor();
        let height = 0.06 + hash(block, 1.0, 21) * 0.07;
        let spire = (1.0 - (along.rem_euclid(1.0) - 0.5).abs() * 2.0).powi(3)
            * 0.06
            * (hash(block, 2.0, 21) > 0.55) as u8 as f32;
        let roofline = HORIZON - height - spire;
        if p.y > roofline && p.x > -0.05 {
            let window = (p.x * 90.0).rem_euclid(1.0) < 0.3
                && (p.y * 70.0).rem_euclid(1.0) < 0.35
                && hash((p.x * 90.0).floor(), (p.y * 70.0).floor(), 22) > 0.88;
            color = if window {
                Rgb::hex(0xffb35c)
            } else {
                Rgb::hex(0x4a2e3e).mix(Rgb::hex(0x7b4a4a), smooth(roofline, HORIZON, p.y))
            };
        }
        if p.y > HORIZON - 0.015 {
            color = Rgb::hex(0x3c2638);
        }
        for i in 0..5 {
            let i = i as f32;
            let x = crossing(p, t, 55.0 + i * 6.0, i * 0.2);
            if bird(p, x, 0.2 + i * 0.03, 0.012, t + i) {
                color = Rgb::hex(0x3a2238);
            }
        }
        color
    };
    if p.y < HORIZON {
        return above(p);
    }
    let mut color = mirror(p, t, HORIZON, Rgb::hex(0x5a3a4a), above);
    // The sun's road on the water.
    let road = smooth(0.08 + (p.y - HORIZON) * 0.4, 0.0, (p.x - sun.0).abs());
    let glint = smooth(0.6, 0.8, noise(p.x * 60.0, p.y * 200.0 - t * 3.0));
    color = color.mix(Rgb::hex(0xffe2a0), road * glint);
    for i in 0..2 {
        let i = i as f32;
        let x = crossing(p, t, 120.0, 0.5 + i * 0.45);
        let (dx, dy) = (p.x - x, p.y - (0.68 + i * 0.12));
        let scale = 1.0 + i * 0.6;
        let hull = dy > 0.0 && dy < 0.012 * scale && dx.abs() < (0.07 - dy * 3.0) * scale;
        let rower = dx.abs() < 0.006 * scale && dy > -0.03 * scale && dy < 0.0;
        if hull || rower {
            color = Rgb::hex(0x2a1826);
        }
    }
    color
}

pub fn ocean_sunset(p: Pt, t: f32) -> Rgb {
    const HORIZON: f32 = 0.56;
    let sun = (0.0, 0.5 + (t * 0.004).sin() * 0.02);
    if p.y < HORIZON {
        let mut color = gradient(
            &[
                (0.0, Rgb::hex(0x1d2550)),
                (0.2, Rgb::hex(0x5b3a78)),
                (0.38, Rgb::hex(0xd8577a)),
                (HORIZON, Rgb::hex(0xffa55c)),
            ],
            p.y,
        );
        color = color.add(Rgb::hex(0xff9a50).scale(glow(p, sun.0, sun.1, 0.5) * 0.5));
        let clouds = smooth(0.55, 0.72, fbm(p.x * 1.6 - t * 0.006, p.y * 7.0));
        let lit = Rgb::hex(0xffb38a).mix(Rgb::hex(0x7a3a6a), smooth(0.2, 0.45, p.y));
        color = color.mix(
            lit,
            clouds * smooth(0.05, 0.25, p.y) * smooth(0.5, 0.35, p.y),
        );
        color = color.mix(Rgb::hex(0xffe7b0), disk(p, sun.0, sun.1, 0.09, 0.006));
        for i in 0..3 {
            let i = i as f32;
            let x = crossing(p, t, 45.0 + i * 9.0, i * 0.33);
            if bird(p, x, 0.22 + i * 0.05, 0.02, t + i) {
                color = Rgb::hex(0x2a1838);
            }
        }
        return color;
    }
    let depth = (p.y - HORIZON) / (1.0 - HORIZON);
    let waves =
        ((p.y - HORIZON).powf(0.7) * 90.0 - t * 1.2 + noise(p.x * 6.0, p.y * 30.0) * 3.0).sin();
    let mut color = Rgb::hex(0x3a2550).mix(Rgb::hex(0x1a1a3a), depth);
    color = color.mix(
        Rgb::hex(0xd8577a),
        smooth(0.4, 1.0, waves) * (1.0 - depth) * 0.5,
    );
    let road = smooth(0.12 + depth * 0.25, 0.0, (p.x - sun.0).abs());
    let glint = smooth(
        0.55,
        0.85,
        noise(p.x * 50.0 + t * 0.5, p.y * 160.0 - t * 2.0),
    );
    color = color.mix(Rgb::hex(0xffd59a), road * glint);
    // Foam where the waves break on the sand in front.
    let shore = 0.93 + (p.x * 6.0 + t * 0.4).sin() * 0.015 + (t * 0.5).sin() * 0.01;
    if p.y > shore {
        color = Rgb::hex(0x6a4058).mix(Rgb::hex(0xffd7c4), smooth(shore + 0.02, shore, p.y));
    }
    color
}

pub fn neon_sunset(p: Pt, t: f32) -> Rgb {
    const HORIZON: f32 = 0.6;
    if p.y < HORIZON {
        let mut color = gradient(
            &[
                (0.0, Rgb::hex(0x0b0824)),
                (0.3, Rgb::hex(0x2e1258)),
                (0.5, Rgb::hex(0x8a1e74)),
                (HORIZON, Rgb::hex(0xff4a8a)),
            ],
            p.y,
        );
        color = color.add(Rgb::hex(0xffffff).scale(stars(p, t, 0.015) * smooth(0.4, 0.0, p.y)));
        // The sun, its lower half cut by bands that widen towards the horizon.
        let center = (0.0, 0.4);
        let sun = disk(p, center.0, center.1, 0.21, 0.004);
        let below = (p.y - center.1 + 0.02).max(0.0);
        let gap = below > 0.0 && (below * 55.0).rem_euclid(1.0) < below * 4.5;
        color = color.add(Rgb::hex(0xff3d8a).scale(glow(p, center.0, center.1, 0.55) * 0.35));
        if sun > 0.5 && !gap {
            color = Rgb::hex(0xfff06a).mix(Rgb::hex(0xff3d8a), smooth(0.2, 0.6, p.y));
        }
        // Mountains with bright ridgelines.
        let ridge = HORIZON - 0.02 - fbm(p.x * 2.5, 4.0).powi(2) * 0.24;
        if p.y > ridge {
            let rim = smooth(ridge + 0.008, ridge, p.y);
            color = Rgb::hex(0x14082a).mix(Rgb::hex(0x22b8ff), rim);
        }
        return color;
    }
    // A grid floor running at the viewer: lines a set width apart on the ground, so
    // perspective packs them towards the horizon.
    let depth = p.y - HORIZON;
    let z = 0.08 / depth.max(1e-3);
    let across = (p.x * z * 6.0).rem_euclid(1.0);
    let along = (z * 1.5 + t * 0.5).rem_euclid(1.0);
    let width = 0.04;
    let line = across.min(1.0 - across) < width || along.min(1.0 - along) < width * 1.5;
    let floor = Rgb::hex(0x0e0420).mix(Rgb::hex(0x1e0838), smooth(0.0, 0.3, depth));
    let haze = smooth(0.08, 0.0, depth);
    let color = if line {
        Rgb::hex(0xff4fd8).mix(Rgb::hex(0xffa0f0), smooth(0.2, 0.4, depth))
    } else {
        floor
    };
    color.mix(Rgb::hex(0xff4a8a), haze * 0.6)
}
