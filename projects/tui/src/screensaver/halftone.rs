//! Turns a painted picture into terminal cells: each cell holds 2 by 4 Braille dots, lit
//! by an ordered dither of the picture's brightness and coloured by what they show, over
//! a deep tone of the same picture.

use ratatui::style::Color;

use super::paint::{Pt, Rgb};

/// The Braille bit of each dot, by column and row within a cell.
const DOTS: [[u32; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];
/// The order dots light in as the picture brightens, by dot row and column, repeating
/// every 8 dots: 64 steps of light without lines or clumps.
const BAYER: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];
/// How much of the picture's own colour shows between the dots, and how dark.
const TONE: f32 = 0.5;
const TONE_DEPTH: f32 = 0.42;

pub struct Cell {
    pub symbol: char,
    pub fg: Color,
    pub bg: Color,
}

/// A frame of `width` by `height` cells, row by row, painted by `paint`.
pub fn render(width: u16, height: u16, background: Rgb, paint: impl Fn(Pt) -> Rgb) -> Vec<Cell> {
    let (w, h) = (u32::from(width), u32::from(height));
    let aspect = w as f32 * 2.0 / (h as f32 * 4.0).max(1.0);
    let scale = (h * 4).max(1) as f32;
    let mut cells = Vec::with_capacity((w * h) as usize);
    for row in 0..h {
        for column in 0..w {
            let mut bits = 0;
            let (mut lit, mut all, mut count) = (Rgb::BLACK, Rgb::BLACK, 0.0);
            for (dx, dots) in DOTS.iter().enumerate() {
                for (dy, bit) in dots.iter().enumerate() {
                    let dot = (column * 2 + dx as u32, row * 4 + dy as u32);
                    let point = Pt {
                        x: (dot.0 as f32 + 0.5) / scale - aspect / 2.0,
                        y: (dot.1 as f32 + 0.5) / scale,
                        aspect,
                        dot,
                    };
                    let color = paint(point);
                    all = all.add(color);
                    let rank = BAYER[(dot.1 % 8) as usize][(dot.0 % 8) as usize];
                    let threshold = (f32::from(rank) + 0.5) / 64.0;
                    if brightness(color) > threshold {
                        bits |= bit;
                        lit = lit.add(color);
                        count += 1.0;
                    }
                }
            }
            let all = all.scale(1.0 / 8.0);
            let bg = background.mix(all.scale(TONE_DEPTH), TONE);
            let fg = if count > 0.0 {
                let lit = lit.scale(1.0 / count);
                // Dots carry the hue at full strength; how many are lit carries the light.
                lit.scale((0.6 + 0.4 * lit.max()) / lit.max().max(1e-3))
            } else {
                bg
            };
            cells.push(Cell {
                symbol: char::from_u32(0x2800 + bits).unwrap_or(' '),
                fg: terminal(fg),
                bg: terminal(bg),
            });
        }
    }
    cells
}

/// How much of a cell a colour lights: saturated colours count as bright as they look.
fn brightness(color: Rgb) -> f32 {
    (0.5 * color.max() + 0.5 * color.luma())
        .clamp(0.0, 1.0)
        .powf(0.8)
}

fn terminal(color: Rgb) -> Color {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::Rgb(channel(color.0), channel(color.1), channel(color.2))
}
