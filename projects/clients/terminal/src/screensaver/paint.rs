//! What the scenes paint with: colours, noise and a few shapes they share.

/// A point of the picture. `y` runs 0..1 from top to bottom; `x` is in the same units,
/// 0 at the middle, so a wider window shows more to the sides instead of stretching.
#[derive(Clone, Copy)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
    /// Width over height: `x` runs from `-aspect / 2` to `aspect / 2`.
    pub aspect: f32,
    /// The dot itself, for things one dot large such as stars.
    pub dot: (u32, u32),
}

impl Pt {
    pub fn at(self, x: f32, y: f32) -> Self {
        Self { x, y, ..self }
    }
}

/// Linear colour, each channel 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub f32, pub f32, pub f32);

impl Rgb {
    pub const BLACK: Self = Self(0.0, 0.0, 0.0);

    pub const fn hex(code: u32) -> Self {
        Self(
            ((code >> 16) & 0xff) as f32 / 255.0,
            ((code >> 8) & 0xff) as f32 / 255.0,
            (code & 0xff) as f32 / 255.0,
        )
    }

    pub fn mix(self, to: Self, amount: f32) -> Self {
        let k = amount.clamp(0.0, 1.0);
        Self(
            self.0 + (to.0 - self.0) * k,
            self.1 + (to.1 - self.1) * k,
            self.2 + (to.2 - self.2) * k,
        )
    }

    pub fn scale(self, k: f32) -> Self {
        Self(self.0 * k, self.1 * k, self.2 * k)
    }

    pub fn add(self, other: Self) -> Self {
        Self(self.0 + other.0, self.1 + other.1, self.2 + other.2)
    }

    pub fn max(self) -> f32 {
        self.0.max(self.1).max(self.2)
    }

    pub fn luma(self) -> f32 {
        0.3 * self.0 + 0.59 * self.1 + 0.11 * self.2
    }
}

/// A colour along stops of (position, colour), positions rising.
pub fn gradient(stops: &[(f32, Rgb)], at: f32) -> Rgb {
    let Some(&(first, mut color)) = stops.first() else {
        return Rgb::BLACK;
    };
    let mut from = first;
    for &(to, next) in stops {
        if at <= to {
            return color.mix(next, (at - from) / (to - from).max(1e-6));
        }
        (from, color) = (to, next);
    }
    color
}

pub fn smooth(from: f32, to: f32, x: f32) -> f32 {
    let k = ((x - from) / (to - from)).clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// A stable pseudo-random value in 0..1 for a point and a stream.
pub fn hash(x: f32, y: f32, stream: u32) -> f32 {
    let mut n = (x as i32 as u32).wrapping_mul(0x9E37_79B1)
        ^ (y as i32 as u32).wrapping_mul(0x85EB_CA77)
        ^ stream.wrapping_mul(0xC2B2_AE3D);
    n ^= n >> 15;
    n = n.wrapping_mul(0x2C1B_3C6D);
    n ^= n >> 12;
    n = n.wrapping_mul(0x297A_2D39);
    n ^= n >> 15;
    (n >> 8) as f32 / (1u32 << 24) as f32
}

/// Smooth value noise in 0..1.
pub fn noise(x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (sx, sy) = (smooth(0.0, 1.0, x - x0), smooth(0.0, 1.0, y - y0));
    let corner = |dx: f32, dy: f32| hash(x0 + dx, y0 + dy, 11);
    let top = corner(0.0, 0.0) + (corner(1.0, 0.0) - corner(0.0, 0.0)) * sx;
    let bottom = corner(0.0, 1.0) + (corner(1.0, 1.0) - corner(0.0, 1.0)) * sx;
    top + (bottom - top) * sy
}

/// Four octaves of noise, in 0..1.
pub fn fbm(x: f32, y: f32) -> f32 {
    let (mut sum, mut weight, mut scale) = (0.0, 0.5, 1.0);
    for _ in 0..4 {
        sum += noise(x * scale, y * scale) * weight;
        scale *= 2.0;
        weight *= 0.5;
    }
    sum / 0.9375
}

/// A soft disk: 1 inside `radius`, fading to 0 over `soft`.
pub fn disk(p: Pt, cx: f32, cy: f32, radius: f32, soft: f32) -> f32 {
    let distance = ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt();
    1.0 - smooth(radius, radius + soft, distance)
}

/// The glow around a light: 1 at its centre, falling off with distance.
pub fn glow(p: Pt, cx: f32, cy: f32, reach: f32) -> f32 {
    let distance = ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt();
    (1.0 - distance / reach).max(0.0).powi(2)
}

/// Stars one dot large; `share` of the dots hold one, each twinkling on its own.
pub fn stars(p: Pt, t: f32, share: f32) -> f32 {
    let (x, y) = (p.dot.0 as f32, p.dot.1 as f32);
    if hash(x, y, 3) > share {
        return 0.0;
    }
    let phase = hash(x, y, 4) * std::f32::consts::TAU;
    (0.55 + 0.45 * (t * (1.0 + hash(x, y, 5) * 2.0) + phase).sin()) * (0.4 + hash(x, y, 6))
}

/// A bird seen from afar, a flapping V of half-width `size` centred at cx, cy.
pub fn bird(p: Pt, cx: f32, cy: f32, size: f32, t: f32) -> bool {
    let dx = (p.x - cx).abs();
    if dx > size {
        return false;
    }
    let flap = (t * 5.0 + cx * 9.0).sin() * 0.5;
    let wing = cy - dx * (0.5 + flap) + (dx / size).powi(2) * size * 0.4;
    (p.y - wing).abs() < size * 0.14 + 0.002
}

/// Where a thing crossing the whole width is now: from just off the left edge to just
/// off the right, in `period` seconds, started at `offset` of the way.
pub fn crossing(p: Pt, t: f32, period: f32, offset: f32) -> f32 {
    let span = p.aspect + 0.4;
    ((t / period + offset).rem_euclid(1.0)) * span - span / 2.0
}

/// Things falling or drifting down, such as snow or leaves: a mask and the hash of the
/// piece, so the caller can colour each its own way.
pub fn falling(p: Pt, t: f32, speed: f32, size: f32, share: f32, stream: u32) -> Option<f32> {
    let cell = size * 6.0;
    let y = p.y - t * speed;
    let column = (p.x / cell).floor();
    let sway = (t * 0.7 + column * 1.7).sin() * cell * 0.25;
    let x = p.x + sway;
    let (cx, cy) = ((x / cell).floor(), (y / cell).floor());
    let seed = hash(cx, cy, stream);
    if seed > share {
        return None;
    }
    let (ox, oy) = (
        (cx + 0.2 + 0.6 * hash(cx, cy, stream + 1)) * cell,
        (cy + 0.2 + 0.6 * hash(cx, cy, stream + 2)) * cell,
    );
    let inside = ((x - ox).powi(2) + (y - oy).powi(2)).sqrt() < size;
    inside.then_some(hash(cx, cy, stream + 3))
}

/// Thin wind-driven veils near a horizon. Two differently paced layers keep the air
/// moving gently without moving the silhouettes behind it.
pub fn mist(p: Pt, t: f32, height: f32, color: Rgb, tint: Rgb, strength: f32) -> Rgb {
    let band = smooth(height - 0.09, height, p.y) * smooth(height + 0.13, height + 0.02, p.y);
    if band == 0.0 {
        return color;
    }
    let drift = noise(p.x * 3.0 - t * 0.012, p.y * 18.0) * 0.65
        + noise(p.x * 7.0 + t * 0.018, p.y * 31.0) * 0.35;
    color.mix(tint, band * smooth(0.28, 0.78, drift) * strength)
}

/// Small tufts rooted in the ground, leaning together in a slow gust. Positions and
/// widths use the same units as scene geometry, independent of terminal size.
pub fn grass(p: Pt, t: f32, ground: f32) -> f32 {
    if p.y <= ground {
        return 0.0;
    }
    let row = (p.y / 0.045).floor();
    let column = (p.x / 0.018).floor();
    let seed = hash(column, row, 101);
    let root = (row + 0.7 + seed * 0.3) * 0.045;
    let up = root - p.y;
    let depth = smooth(ground, 1.0, p.y);
    let height = (0.012 + seed * 0.023) * (0.5 + depth * 0.5);
    if up <= 0.0 || up >= height {
        return 0.0;
    }
    let gust = (t * 0.8 + p.x * 4.0 + row * 0.25).sin();
    let lean = (0.2 + gust * 0.3) * up;
    let x = (column + 0.2 + hash(column, row, 102) * 0.6) * 0.018;
    let width = 0.0015 * (1.0 - up / height) + 0.0005;
    (1.0 - smooth(width, width * 2.0, (p.x - x - lean).abs())) * (0.4 + depth * 0.6)
}

/// Sparse slanted rain, with a dim distant layer and a longer foreground layer.
/// Each streak moves continuously; no frame-dependent random flashing.
pub fn rain(p: Pt, t: f32) -> f32 {
    let mut light: f32 = 0.0;
    for (scale, speed, length, strength, stream) in [
        (0.07, 0.11, 0.009, 0.35, 111),
        (0.11, 0.18, 0.018, 0.65, 112),
    ] {
        let y = p.y - t * speed;
        let x = p.x - p.y * 0.18;
        let (cx, cy) = ((x / scale).floor(), (y / scale).floor());
        if hash(cx, cy, stream) > 0.13 {
            continue;
        }
        let dx = x - (cx + 0.2 + hash(cx, cy, stream + 10) * 0.6) * scale;
        let dy = y - (cy + 0.2 + hash(cx, cy, stream + 20) * 0.6) * scale;
        let streak = smooth(0.002, 0.0, dx.abs()) * smooth(length, 0.0, dy.abs());
        light = light.max(streak * strength);
    }
    light
}

/// Brief expanding rings on water, flattened by perspective and faded before they
/// restart. They share a continuous clock with the rain and reflections.
pub fn ripples(p: Pt, t: f32, horizon: f32) -> f32 {
    if p.y <= horizon + 0.025 {
        return 0.0;
    }
    let (cx, cy) = ((p.x / 0.16).floor(), (p.y / 0.08).floor());
    let seed = hash(cx, cy, 121);
    if seed > 0.28 {
        return 0.0;
    }
    let phase = (t * 0.23 + seed * 7.0).rem_euclid(1.0);
    let x = (cx + 0.25 + hash(cx, cy, 122) * 0.5) * 0.16;
    let y = (cy + 0.25 + hash(cx, cy, 123) * 0.5) * 0.08;
    let depth = smooth(horizon, 1.0, p.y);
    let radius = phase * (0.014 + depth * 0.025);
    let distance = ((p.x - x).powi(2) + ((p.y - y) * 3.0).powi(2)).sqrt();
    (1.0 - smooth(0.001, 0.004, (distance - radius).abs()))
        * smooth(0.0, 0.15, phase)
        * smooth(1.0, 0.35, phase)
}
