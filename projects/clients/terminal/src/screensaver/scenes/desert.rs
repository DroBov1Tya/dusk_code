//! Desert scenes: a camp under the Milky Way, dunes in the wind, a campfire and a
//! caravan at dusk.

use crate::screensaver::paint::{
    Pt, Rgb, crossing, disk, fbm, glow, gradient, grass, hash, mist, noise, smooth, stars,
};

/// A dune line at `base`, swelling by `height`, drifting with the wind.
fn dune(x: f32, base: f32, height: f32, stretch: f32, seed: f32) -> f32 {
    base - height * (0.5 + 0.5 * (x * stretch + seed).sin()) * (0.6 + 0.4 * noise(x * 2.0, seed))
}

/// A night sky: deep blue, a band of the Milky Way and twinkling stars.
fn night(p: Pt, t: f32, band: bool) -> Rgb {
    let mut color = gradient(
        &[
            (0.0, Rgb::hex(0x060818)),
            (0.6, Rgb::hex(0x131a3a)),
            (0.8, Rgb::hex(0x2a2a4a)),
        ],
        p.y,
    );
    if band {
        // A diagonal band, brightest along its spine, broken by dark dust.
        let across = p.y - (0.7 - p.x * 0.45);
        let spine = (-across * across * 60.0).exp();
        let dust = smooth(0.45, 0.7, fbm(p.x * 6.0 + 3.0, p.y * 6.0));
        let cloud = fbm(p.x * 3.0, p.y * 3.0) * spine * (1.0 - dust * 0.8);
        color = color.add(Rgb::hex(0xb4a8e0).scale(cloud * 0.55));
        color = color.add(Rgb::hex(0xffd3a8).scale(spine * cloud * 0.25));
        color = color.add(Rgb::hex(0xffffff).scale(stars(p, t, 0.012 + spine * 0.05)));
    } else {
        color = color.add(Rgb::hex(0xffffff).scale(stars(p, t, 0.02)));
    }
    color
}

pub fn milky_way_camp(p: Pt, t: f32) -> Rgb {
    let mut color = night(p, t, true);
    let far = dune(p.x, 0.74, 0.08, 3.0, 1.0);
    let near = dune(p.x, 0.86, 0.1, 2.0, 4.0);
    let tent = (0.25, 0.8);
    let lamp = glow(p, tent.0, tent.1, 0.35);
    if p.y > far {
        color = Rgb::hex(0x1a1830).mix(Rgb::hex(0x2c2440), smooth(far, far + 0.06, p.y));
    }
    if p.y > near {
        color = Rgb::hex(0x241a2a).add(Rgb::hex(0x6a3a1a).scale(lamp));
    }
    color = mist(p, t, far, color, Rgb::hex(0x5a4c64), 0.15);
    // A tent lit from inside, its door open.
    let (dx, dy) = (p.x - tent.0, tent.1 - p.y);
    if dy > 0.0 && dy < 0.09 - dx.abs() * 0.9 {
        let door = dx.abs() < 0.03 - dy * 0.3 && dy < 0.06;
        color = if door {
            Rgb::hex(0xffd27a)
        } else {
            Rgb::hex(0xb86a3a).mix(Rgb::hex(0x5a3020), smooth(-0.02, 0.06, dx))
        };
    }
    color.add(Rgb::hex(0xff9a40).scale(lamp * 0.25))
}

pub fn dunes(p: Pt, t: f32) -> Rgb {
    let sun = (-0.55, 0.36);
    let mut color = gradient(
        &[
            (0.0, Rgb::hex(0x22306a)),
            (0.22, Rgb::hex(0x7a5a8a)),
            (0.36, Rgb::hex(0xe8907a)),
            (0.46, Rgb::hex(0xffd09a)),
        ],
        p.y,
    );
    color = color.add(Rgb::hex(0xfff0c8).scale(glow(p, sun.0, sun.1, 0.5) * 0.45));
    color = color.mix(Rgb::hex(0xfffbea), disk(p, sun.0, sun.1, 0.045, 0.012));
    // Ranges of dunes, nearer ones taller and warmer. Each crest is a knife edge: the
    // side towards the sun lit, the lee side in shade, their border curving down from
    // the crest.
    let ranges = [
        (0.46, 0.04, 1.6, 0.3),
        (0.56, 0.08, 1.0, 1.9),
        (0.72, 0.13, 0.7, 3.6),
        (0.98, 0.2, 0.45, 5.2),
    ];
    for (index, (base, height, stretch, seed)) in ranges.into_iter().enumerate() {
        let bend = noise(p.x * stretch * 0.7, seed) * 1.4;
        let phase = p.x * stretch * std::f32::consts::TAU + seed + bend;
        let swell = 0.5 + 0.5 * phase.sin();
        let crest = base - height * swell.powf(1.4);
        if p.y <= crest {
            continue;
        }
        let near = index as f32 / 3.0;
        let below = p.y - crest;
        let lee = (phase + below * (2.5 + near * 1.5)).cos();
        // Only the slope under a crest falls into shade, never the flat between dunes.
        let tall = height * swell.powf(1.4) + 0.015;
        let lit = smooth(-0.5, -0.3, lee)
            .max(smooth(0.3, 0.05, swell))
            .max(smooth(tall * 0.8, tall * 1.3, below));
        let shade = Rgb::hex(0x6a3a5a).mix(Rgb::hex(0x3a1a2e), near);
        let light = Rgb::hex(0xf0a070).mix(Rgb::hex(0xff8c3a), near);
        let mut side = shade.mix(light, lit);
        side = side.scale(1.0 - smooth(0.0, 0.35, below) * 0.25);
        side = side.mix(
            Rgb::hex(0xffe0a0),
            smooth(0.008, 0.0, below) * (0.3 + lit * 0.7),
        );
        // Wind ripples on the lit sand and grains blown off the crest.
        let ripple = (p.y * 320.0 + p.x * 50.0 + noise(p.x * 6.0, p.y * 6.0) * 8.0).sin();
        side = side.scale(1.0 - smooth(0.6, 1.0, ripple) * 0.12 * near * lit);
        let drift = smooth(0.6, 0.85, noise(p.x * 25.0 - t * 2.5, p.y * 50.0));
        side = side.mix(Rgb::hex(0xffe8c0), drift * smooth(0.02, 0.0, below) * 0.8);
        color = side.mix(Rgb::hex(0xa86a78), (1.0 - near) * 0.5);
        color = mist(
            p,
            t,
            crest + 0.015,
            color,
            Rgb::hex(0xc18b8a),
            (1.0 - near) * 0.1,
        );
    }
    color
}

pub fn campfire(p: Pt, t: f32) -> Rgb {
    let fire = (0.0, 0.8);
    let light = glow(p, fire.0, fire.1, 0.6);
    let mut color = night(p, t, false);
    // Pines against the sky, ragged and uneven.
    let along = p.x * 11.0;
    let column = along.floor();
    let height = 0.12 + hash(column, 1.0, 33) * 0.14;
    let half = (along.rem_euclid(1.0) - 0.5).abs();
    let tiers = ((0.62 - p.y) * 60.0).rem_euclid(1.0) * 0.12;
    let pine = p.y > 0.62 - height && half < (p.y - 0.62 + height) / height * (0.42 + tiers);
    if pine || p.y > 0.62 - fbm(p.x * 4.0, 2.0) * 0.04 {
        color = Rgb::hex(0x070a10).add(Rgb::hex(0x3a1a08).scale(light * 0.45));
    }
    if p.y > 0.78 {
        color = Rgb::hex(0x1a1410).add(Rgb::hex(0xa04a10).scale(light.powf(1.5)));
        let blades = grass(p, t, 0.78);
        let meadow = Rgb::hex(0x193132).add(Rgb::hex(0x9b642b).scale(light * 0.55));
        color = color.mix(meadow, blades * 0.6);
    }
    color = mist(p, t, 0.66, color, Rgb::hex(0x34444b), 0.13);
    // Smoke curls into the cold air behind the flames.
    let up = fire.1 - p.y;
    if up > 0.1 && up < 0.6 {
        let sway = (up * 9.0 - t * 0.3).sin() * up * 0.08;
        let plume = smooth(0.02 + up * 0.12, 0.0, (p.x - fire.0 - sway).abs());
        color = color.mix(
            Rgb::hex(0x48404b),
            plume * noise(p.x * 25.0, up * 14.0 - t * 0.4) * 0.22,
        );
    }
    // Logs, then flames rising from them, licking upwards.
    let (dx, dy) = (p.x - fire.0, fire.1 - p.y);
    let logs = dy > -0.02
        && dy < 0.01
        && dx.abs() < 0.09
        && ((dx * 2.0 + dy * 9.0).abs() < 0.03 || (dx * 2.0 - dy * 9.0).abs() < 0.03);
    if logs {
        color = Rgb::hex(0x4a2a18);
    }
    if dy > 0.0 {
        let flicker = noise(dx * 30.0, dy * 12.0 - t * 4.0);
        let width = 0.1 * (1.0 - dy / 0.3).max(0.0).powf(0.8) * (0.55 + flicker * 0.9);
        let heat = 1.0 - (dx.abs() / width.max(1e-3));
        if heat > 0.0 {
            let flame = Rgb::hex(0xe8300c).mix(Rgb::hex(0xffe48a), heat * (1.0 - dy * 2.5));
            color = color.mix(flame, smooth(0.0, 0.3, heat));
        }
        // Sparks climbing in the warm air.
        let column = (dx * 60.0).floor();
        let rise = dy * 8.0 + t * (0.6 + hash(column, 1.0, 31));
        if hash(column, rise.floor(), 32) > 0.93 && rise.rem_euclid(1.0) < 0.15 && dy < 0.45 {
            color = Rgb::hex(0xffb84a).scale(1.0 - dy * 1.5);
        }
    }
    color.add(Rgb::hex(0xff7a20).scale(light * 0.12))
}

pub fn caravan(p: Pt, t: f32) -> Rgb {
    let sun = (-0.2, 0.55);
    let mut color = gradient(
        &[
            (0.0, Rgb::hex(0x2a1a48)),
            (0.3, Rgb::hex(0x8a3a6a)),
            (0.55, Rgb::hex(0xff7a4a)),
            (0.7, Rgb::hex(0xffb86a)),
        ],
        p.y,
    );
    color = color.mix(Rgb::hex(0xffe0a0), disk(p, sun.0, sun.1, 0.16, 0.006));
    color = color.add(Rgb::hex(0xff8a4a).scale(glow(p, sun.0, sun.1, 0.6) * 0.3));
    let horizon = dune(p.x, 0.72, 0.05, 2.5, 0.7);
    if p.y > horizon {
        color = Rgb::hex(0x3a1a2a).mix(Rgb::hex(0x5a2a30), smooth(horizon, 1.0, p.y));
    }
    color = mist(p, t, horizon + 0.01, color, Rgb::hex(0x9d6867), 0.15);
    // Camels with riders along the crest, walking to the right.
    for i in 0..4 {
        let i = i as f32;
        let x = crossing(p, t, 160.0, 0.55) - i * 0.22;
        let ground = dune(x, 0.72, 0.05, 2.5, 0.7);
        if camel(p, x, ground, 0.085, t + i * 1.3) {
            color = Rgb::hex(0x1e0e1c);
        }
    }
    color
}

/// A camel side on, walking right, feet at `ground`, `size` tall at the hump.
fn camel(p: Pt, x: f32, ground: f32, size: f32, t: f32) -> bool {
    let (dx, dy) = ((p.x - x) / size, (ground - p.y) / size);
    let body = (dx / 0.55).powi(2) + ((dy - 0.62) / 0.2).powi(2) < 1.0;
    let hump = (dx / 0.28).powi(2) + ((dy - 0.8) / 0.18).powi(2) < 1.0;
    let neck = dx > 0.4 && dx < 0.75 && (dy - (0.55 + (dx - 0.4) * 1.2)).abs() < 0.08;
    let head = ((dx - 0.82) / 0.13).powi(2) + ((dy - 0.98) / 0.07).powi(2) < 1.0;
    let rider = dx.abs() < 0.08 && dy > 0.8 && dy < 1.15;
    let stride = (t * 3.0).sin() * 0.12;
    let leg = |at: f32, swing: f32| {
        let foot = at + swing;
        let lean = at + (foot - at) * (1.0 - dy / 0.5);
        dy > 0.0 && dy < 0.5 && (dx - lean).abs() < 0.04
    };
    body || hump
        || neck
        || head
        || rider
        || leg(-0.38, stride)
        || leg(-0.25, -stride)
        || leg(0.3, -stride)
        || leg(0.42, stride)
}
