//! The loader on a subagent card: a few dots chase each other along a random path in a
//! small grid, in colors of their own. The agent's thread seeds both, so an agent keeps
//! its loader, and no two look alike.

use ratatui::{style::Color, text::Span};

use super::theme::fg;

const WIDTH: usize = 5;
const HEIGHT: usize = 3;
/// The longest path; a walk that gets stuck ends sooner.
const PATH_LENGTH: usize = 8;
/// A shorter path barely moves.
const SHORTEST_PATH: usize = 4;
/// The head and the dots that trail it, brightest first.
const DOTS: [&str; 3] = ["\u{25CF}", "\u{2022}", "\u{00B7}"];
/// Screen frames per step of the dots.
const FRAMES_PER_STEP: usize = 2;

pub struct Loader {
    path: Vec<(usize, usize)>,
    head: Color,
    tail: Color,
}

impl Loader {
    pub fn new(seed: u64) -> Self {
        let mut random = Random(seed);
        let mut path = walk(&mut random);
        while path.len() < SHORTEST_PATH {
            path = walk(&mut random);
        }
        let hue = random.below(360) as f64;
        let shift = 60.0 + random.below(120) as f64;
        Self {
            path,
            head: hsv(hue, 0.6, 1.0),
            tail: hsv(hue + shift, 0.7, 0.7),
        }
    }

    /// The color names and accents take.
    pub fn color(&self) -> Color {
        self.head
    }

    /// Rows of the grid. `Some(frame)` moves the dots; `None` draws the whole path at
    /// rest, for an agent that finished.
    pub fn rows(&self, frame: Option<usize>) -> [Vec<Span<'static>>; HEIGHT] {
        let mut cells = [[None; WIDTH]; HEIGHT];
        match frame {
            Some(frame) => {
                let step = frame / FRAMES_PER_STEP;
                // Oldest first, so the head is drawn over its trail.
                for age in (0..DOTS.len()).rev() {
                    let Some(step) = step.checked_sub(age) else {
                        continue;
                    };
                    let (x, y) = self.path[self.bounce(step)];
                    cells[y][x] = Some((DOTS[age], self.shade(age)));
                }
            }
            None => {
                for &(x, y) in &self.path {
                    cells[y][x] = Some((DOTS[2], self.tail));
                }
            }
        }
        cells.map(|row| {
            row.into_iter()
                .map(|cell| match cell {
                    Some((dot, color)) => Span::styled(dot, fg(color)),
                    None => Span::raw(" "),
                })
                .collect()
        })
    }

    /// There and back along the path, so the dots never jump.
    fn bounce(&self, step: usize) -> usize {
        let last = self.path.len() - 1;
        let at = step % (2 * last);
        if at <= last { at } else { 2 * last - at }
    }

    fn shade(&self, age: usize) -> Color {
        let share = age as f64 / (DOTS.len() - 1) as f64;
        blend(self.head, self.tail, share)
    }
}

/// The core's agent ID is stable; its mixed bits choose a name, colour and geometry.
pub fn identity(id: &str) -> u64 {
    id.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

pub fn agent_name(seed: u64) -> String {
    const NAMES: [&str; 8] = [
        "Aster", "Beryl", "Cinder", "Dapple", "Ember", "Fable", "Glint", "Hollow",
    ];
    format!(
        "{}-{:02x}",
        NAMES[(seed as usize) % NAMES.len()],
        (seed >> 24) & 0xff
    )
}

/// A walk through neighbouring cells, diagonals included, that never crosses itself.
fn walk(random: &mut Random) -> Vec<(usize, usize)> {
    let mut path = vec![(random.below(WIDTH), random.below(HEIGHT))];
    while path.len() < PATH_LENGTH {
        let (x, y) = path[path.len() - 1];
        let free: Vec<(usize, usize)> = neighbours(x, y)
            .filter(|cell| !path.contains(cell))
            .collect();
        if free.is_empty() {
            break;
        }
        path.push(free[random.below(free.len())]);
    }
    path
}

fn neighbours(x: usize, y: usize) -> impl Iterator<Item = (usize, usize)> {
    (-1i64..=1).flat_map(move |dy| {
        (-1i64..=1).filter_map(move |dx| {
            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
            let inside = (0..WIDTH as i64).contains(&nx) && (0..HEIGHT as i64).contains(&ny);
            (inside && (dx, dy) != (0, 0)).then_some((nx as usize, ny as usize))
        })
    })
}

/// SplitMix64: small and good enough to tell agents apart, not for anything secret.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn hsv(hue: f64, saturation: f64, value: f64) -> Color {
    let hue = hue.rem_euclid(360.0) / 60.0;
    let chroma = value * saturation;
    let second = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (r, g, b) = match hue as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let base = value - chroma;
    let channel = |part: f64| ((part + base) * 255.0).round() as u8;
    Color::Rgb(channel(r), channel(g), channel(b))
}

fn blend(from: Color, to: Color, share: f64) -> Color {
    match (from, to) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * share).round() as u8;
            Color::Rgb(mix(r1, r2), mix(g1, g2), mix(b1, b2))
        }
        _ => from,
    }
}
