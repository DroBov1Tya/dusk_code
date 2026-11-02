//! The idle screen: after a while with no input and no work (`appearance` in the config,
//! five minutes by default), a random calm scene plays in a window over the app, a margin
//! of the app still showing around it. Any key or click puts the app back.
//!
//! Scenes are drawn from the time alone, cell by cell, so they need no frames on disk
//! and fit any window size.

use std::{
    f32::consts::TAU,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, BorderType, Clear},
};

use super::theme;

/// What stays of the app around the window, in cells.
const MARGIN_X: u16 = 3;
const MARGIN_Y: u16 = 1;
const FPS: u32 = 12;
/// Terminal cells are about twice as tall as wide; round shapes are stretched by this.
const ASPECT: f32 = 2.0;
const BACKGROUND: Color = theme::INPUT_BG;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scene {
    Sea,
    Rain,
    Aurora,
    Stars,
    Embers,
    Lava,
}

impl Scene {
    const ALL: [Self; 6] = [
        Self::Sea,
        Self::Rain,
        Self::Aurora,
        Self::Stars,
        Self::Embers,
        Self::Lava,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Sea => "moonlit sea",
            Self::Rain => "rain",
            Self::Aurora => "aurora",
            Self::Stars => "drifting stars",
            Self::Embers => "embers",
            Self::Lava => "lava lamp",
        }
    }
}

pub struct Playing {
    scene: Scene,
    started: Instant,
}

impl Playing {
    pub fn random() -> Option<Self> {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .subsec_nanos();
        Some(Self {
            scene: Scene::ALL[seed as usize % Scene::ALL.len()],
            started: Instant::now(),
        })
    }

    pub fn frame_period(&self) -> Duration {
        Duration::from_secs(1) / FPS
    }

    pub fn draw(&self, frame: &mut Frame) {
        let screen = frame.area();
        let area = Rect {
            x: screen.x + MARGIN_X,
            y: screen.y + MARGIN_Y,
            width: screen.width.saturating_sub(2 * MARGIN_X),
            height: screen.height.saturating_sub(2 * MARGIN_Y),
        };
        if area.width < 3 || area.height < 3 {
            return;
        }
        let window = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(theme::NIGHT))
            .title(format!(" {} ", self.scene.name()))
            .title_style(Style::new().fg(theme::HAZE))
            .style(Style::new().bg(BACKGROUND));
        let inner = window.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(window, area);
        let t = self.started.elapsed().as_secs_f32();
        let cells = render(self.scene, inner.width, inner.height, t);
        let buffer = frame.buffer_mut();
        for (index, (symbol, color)) in cells.into_iter().enumerate() {
            let x = inner.x + (index % usize::from(inner.width)) as u16;
            let y = inner.y + (index / usize::from(inner.width)) as u16;
            if symbol != ' '
                && let Some(cell) = buffer.cell_mut((x, y))
            {
                cell.set_char(symbol).set_fg(color).set_bg(BACKGROUND);
            }
        }
    }
}

/// One frame, row by row; a space is left blank.
fn render(scene: Scene, width: u16, height: u16, t: f32) -> Vec<(char, Color)> {
    let (w, h) = (usize::from(width), usize::from(height));
    let mut canvas = Canvas {
        w,
        h,
        cells: vec![(' ', BACKGROUND); w * h],
    };
    match scene {
        Scene::Sea => sea(&mut canvas, t),
        Scene::Rain => rain(&mut canvas, t),
        Scene::Aurora => aurora(&mut canvas, t),
        Scene::Stars => stars(&mut canvas, t),
        Scene::Embers => embers(&mut canvas, t),
        Scene::Lava => lava(&mut canvas, t),
    }
    canvas.cells
}

struct Canvas {
    w: usize,
    h: usize,
    cells: Vec<(char, Color)>,
}

impl Canvas {
    fn set(&mut self, x: f32, y: f32, symbol: char, color: Color) {
        if x < 0.0 || y < 0.0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x < self.w && y < self.h {
            self.cells[y * self.w + x] = (symbol, color);
        }
    }

    /// Every cell, with its position and the window's size.
    fn each(&mut self, mut paint: impl FnMut(f32, f32, f32, f32) -> Option<(char, Color)>) {
        let (w, h) = (self.w as f32, self.h as f32);
        for y in 0..self.h {
            for x in 0..self.w {
                if let Some(cell) = paint(x as f32, y as f32, w, h) {
                    self.cells[y * self.w + x] = cell;
                }
            }
        }
    }
}

/// Faint stars that twinkle, above `below` of the height.
fn twinkle(canvas: &mut Canvas, t: f32, below: f32) {
    canvas.each(|x, y, _, h| {
        if y > h * below || hash(x, y, 1) < 0.985 {
            return None;
        }
        let shine = (t * 1.3 + hash(x, y, 2) * TAU).sin();
        (shine > -0.2).then(|| {
            let symbol = if shine > 0.8 { '*' } else { '\u{00B7}' };
            (
                symbol,
                mix(theme::NIGHT, theme::SIDEBAR_FG, 0.4 + 0.5 * shine.max(0.0)),
            )
        })
    });
}

fn sea(canvas: &mut Canvas, t: f32) {
    twinkle(canvas, t, 0.55);
    canvas.each(|x, y, w, h| {
        let horizon = h * 0.6;
        let (mx, my, radius) = (w * 0.68, h * 0.2, (h * 0.07).max(1.0));
        let moon = ((x - mx) / ASPECT).hypot(y - my);
        let pale = Color::Rgb(246, 232, 176);
        if y < horizon {
            return match moon {
                d if d < radius => Some(('\u{2588}', pale)),
                d if d < radius + 0.8 => Some(('\u{2593}', mix(BACKGROUND, pale, 0.5))),
                d if d < radius + 1.8 => Some(('\u{2591}', mix(BACKGROUND, pale, 0.22))),
                _ => None,
            };
        }
        let depth = (y - horizon) / (h - horizon).max(1.0);
        let swell = noise(x * 0.09 + t * 0.35, (y - horizon) * 0.6 - t * 0.25);
        // The moon's path widens toward the shore and breaks on the swell.
        let path = (x - mx).abs() < 1.5 + depth * w * 0.12 && swell > 0.45;
        if path {
            let symbol = if swell > 0.6 { '\u{2550}' } else { '-' };
            return Some((symbol, mix(theme::NIGHT, pale, 0.9 - depth * 0.4)));
        }
        let sea = mix(Color::Rgb(26, 48, 92), Color::Rgb(70, 110, 170), swell);
        match swell {
            s if s > 0.68 => Some(('~', sea)),
            s if s > 0.5 => Some(('\u{223C}', mix(BACKGROUND, sea, 0.7))),
            s if s > 0.35 => Some(('\u{00B7}', mix(BACKGROUND, sea, 0.5))),
            _ => None,
        }
    });
}

fn rain(canvas: &mut Canvas, t: f32) {
    let h = canvas.h as f32;
    let drop = Color::Rgb(120, 150, 200);
    for column in 0..canvas.w {
        let x = column as f32;
        if hash(x, 0.0, 3) < 0.55 {
            continue;
        }
        let speed = 9.0 + hash(x, 0.0, 4) * 12.0;
        let fall = h + 6.0;
        let head = (t * speed + hash(x, 0.0, 5) * fall) % fall;
        for tail in 0..4 {
            let fade = 1.0 - tail as f32 / 4.0;
            canvas.set(
                x,
                head - tail as f32,
                '\u{2502}',
                mix(BACKGROUND, drop, fade * 0.9),
            );
        }
        // A splash where the drop lands.
        if head >= h - 1.0 && head < h + 1.0 {
            canvas.set(x - 1.0, h - 1.0, '.', drop);
            canvas.set(x + 1.0, h - 1.0, '.', drop);
        }
    }
    canvas.each(|x, y, _, h| {
        let puddle = y >= h - 1.0 && noise(x * 0.3 + t * 0.5, 3.0) > 0.55;
        puddle.then(|| ('_', mix(BACKGROUND, Color::Rgb(90, 110, 150), 0.6)))
    });
}

fn aurora(canvas: &mut Canvas, t: f32) {
    twinkle(canvas, t, 0.8);
    canvas.each(|x, y, _, h| {
        let peaks = noise(x * 0.05, 7.0) * 0.65 + noise(x * 0.17, 9.0) * 0.35;
        let ridge = h * 0.95 - peaks * h * 0.28;
        if y > ridge {
            return Some(('\u{2593}', Color::Rgb(22, 20, 32)));
        }
        let center = h * 0.35
            + (x * 0.06 + t * 0.35).sin() * h * 0.1
            + (noise(x * 0.03 + t * 0.1, 1.0) - 0.5) * h * 0.2;
        let spread = (h * 0.16).max(1.0);
        let band = (-((y - center) / spread).powi(2)).exp();
        let curtain = noise(x * 0.15 + t * 0.25, t * 0.15);
        let light = band * (0.3 + 0.7 * curtain);
        let color = mix(theme::TEAL, theme::VIOLET, (y / h * 1.6).min(1.0));
        match light {
            l if l > 0.75 => Some(('\u{2503}', color)),
            l if l > 0.5 => Some(('\u{2502}', mix(BACKGROUND, color, 0.8))),
            l if l > 0.3 => Some((':', mix(BACKGROUND, color, 0.55))),
            l if l > 0.18 => Some(('.', mix(BACKGROUND, color, 0.35))),
            _ => None,
        }
    });
}

fn stars(canvas: &mut Canvas, t: f32) {
    let (w, h) = (canvas.w as f32, canvas.h as f32);
    for (layer, (speed, symbol, light)) in
        [(0.8, '.', 0.35), (2.0, '\u{00B7}', 0.6), (4.5, '*', 0.95)]
            .into_iter()
            .enumerate()
    {
        let count = (w * h / 60.0) as usize / (layer + 1);
        for star in 0..count {
            let seed = star as f32;
            let x = (hash(seed, layer as f32, 6) * w - t * speed).rem_euclid(w);
            let y = hash(seed, layer as f32, 7) * h;
            canvas.set(x, y, symbol, mix(BACKGROUND, theme::SIDEBAR_FG, light));
        }
    }
    // Now and then a meteor crosses.
    let period = 9.0;
    let age = t % period;
    if age < 1.2 {
        let start = hash((t / period).floor(), 0.0, 8);
        for trail in 0..6 {
            let along = age * 40.0 - trail as f32 * 1.5;
            let (x, y) = (w * start + along * 1.6, h * 0.15 + along * 0.4);
            let fade = 1.0 - trail as f32 / 6.0;
            canvas.set(x, y, '\u{2572}', mix(BACKGROUND, theme::SUN, fade));
        }
    }
}

fn embers(canvas: &mut Canvas, t: f32) {
    let (w, h) = (canvas.w as f32, canvas.h as f32);
    canvas.each(|x, y, _, h| {
        let flame = noise(x * 0.2, t * 1.5) * 3.0;
        let depth = h - 1.0 - y;
        (depth < flame).then(|| {
            let heat = 1.0 - depth / 3.0;
            let symbol = ['\u{2581}', '\u{2583}', '\u{2585}'][(heat * 2.9) as usize % 3];
            (symbol, mix(theme::EMBER, theme::SUN, heat))
        })
    });
    let count = (w / 2.0) as usize + 8;
    for ember in 0..count {
        let seed = ember as f32;
        let period = 5.0 + hash(seed, 0.0, 9) * 6.0;
        let age = (t + hash(seed, 1.0, 9) * period) % period / period;
        let x = hash(seed, 2.0, 9) * w + (age * 9.0 + seed).sin() * 2.0;
        let y = h - 2.0 - age * h;
        let symbol = if age < 0.3 {
            '\u{2022}'
        } else if age < 0.7 {
            '\u{00B7}'
        } else {
            '.'
        };
        let color = mix(
            mix(theme::SUN, theme::EMBER, age),
            BACKGROUND,
            (age - 0.5).max(0.0),
        );
        canvas.set(x, y, symbol, color);
    }
}

fn lava(canvas: &mut Canvas, t: f32) {
    let (w, h) = (canvas.w as f32, canvas.h as f32);
    let blobs: Vec<(f32, f32, f32)> = (0..5)
        .map(|blob| {
            let seed = blob as f32;
            let speed = 0.08 + hash(seed, 0.0, 10) * 0.08;
            let x = w * (0.5 + 0.35 * (t * speed * 1.3 + seed * 2.1).sin());
            let y = h * (0.5 + 0.4 * (t * speed + seed * 1.7).cos());
            (
                x,
                y,
                (h * 0.14).max(1.5) * (0.7 + hash(seed, 1.0, 10) * 0.6),
            )
        })
        .collect();
    canvas.each(|x, y, _, h| {
        let field: f32 = blobs
            .iter()
            .map(|&(bx, by, r)| r * r / (((x - bx) / ASPECT).powi(2) + (y - by).powi(2) + 0.5))
            .sum();
        let color = mix(theme::ROSE, theme::VIOLET, y / h);
        match field {
            f if f > 2.2 => Some(('\u{2588}', color)),
            f if f > 1.4 => Some(('\u{2593}', mix(BACKGROUND, color, 0.85))),
            f if f > 1.0 => Some(('\u{2592}', mix(BACKGROUND, color, 0.6))),
            f if f > 0.75 => Some(('\u{2591}', mix(BACKGROUND, color, 0.35))),
            _ => None,
        }
    });
}

/// A stable pseudo-random value in 0..1 for a point and a stream.
fn hash(x: f32, y: f32, stream: u32) -> f32 {
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
fn noise(x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let smooth = |v: f32| v * v * (3.0 - 2.0 * v);
    let (sx, sy) = (smooth(fx), smooth(fy));
    let corner = |dx: f32, dy: f32| hash(x0 + dx, y0 + dy, 11);
    let top = corner(0.0, 0.0) + (corner(1.0, 0.0) - corner(0.0, 0.0)) * sx;
    let bottom = corner(0.0, 1.0) + (corner(1.0, 1.0) - corner(0.0, 1.0)) * sx;
    top + (bottom - top) * sy
}

fn mix(from: Color, to: Color, amount: f32) -> Color {
    let rgb = |color| match color {
        Color::Rgb(r, g, b) => (f32::from(r), f32::from(g), f32::from(b)),
        _ => (128.0, 128.0, 128.0),
    };
    let (a, b, k) = (rgb(from), rgb(to), amount.clamp(0.0, 1.0));
    let channel = |a: f32, b: f32| (a + (b - a) * k).round() as u8;
    Color::Rgb(channel(a.0, b.0), channel(a.1, b.1), channel(a.2, b.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_draws_at_any_size() {
        for scene in Scene::ALL {
            for (width, height) in [(1, 1), (7, 3), (120, 40)] {
                for t in [0.0, 3.7, 600.0] {
                    let cells = render(scene, width, height, t);
                    assert_eq!(cells.len(), usize::from(width) * usize::from(height));
                }
            }
            let shown = render(scene, 120, 40, 2.0)
                .iter()
                .filter(|(c, _)| *c != ' ')
                .count();
            assert!(shown > 20, "{scene:?} drew only {shown} cells");
        }
    }
}
