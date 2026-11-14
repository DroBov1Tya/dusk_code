//! The coarser, more graphic looks: braille dots lit by an ordered dither, and an ASCII
//! ramp of glyphs that fill more or less of the cell. Both look at a cell in 2 by 4
//! places and draw it in the picture's own hue over a deep tone of the same.

use super::{
    cell::{Cell, terminal},
    paint::{Pt, Rgb},
};

const PLACES: (usize, usize) = (2, 4);
/// Braille bits by column and row, and the order the dots of a cell light up in.
const BITS: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];
const ORDER: [[u8; 4]; 2] = [[0, 6, 1, 7], [4, 2, 5, 3]];
/// Glyphs by how much of a cell they fill, the dark end first.
const RAMP: &str = " .:-=+*#%@";
/// How much of the picture's colour shows between the dots, and how dark it is.
/// How much brighter a place counts for the dither: dark pictures keep their shapes.
const GAIN: f32 = 1.6;
const TONE: f32 = 0.45;
const TONE_DEPTH: f32 = 0.35;

/// The places of one cell, row by row, painted by `paint`.
fn places(width: u16, height: u16, paint: impl Fn(Pt) -> Rgb, mut each: impl FnMut([[Rgb; 4]; 2])) {
    let (w, h) = (usize::from(width), usize::from(height));
    let aspect = w as f32 / (2.0 * h.max(1) as f32);
    let (across, down) = (w * PLACES.0, h * PLACES.1);
    for row in 0..h {
        for column in 0..w {
            let mut cell = [[Rgb::BLACK; 4]; 2];
            for (dx, line) in cell.iter_mut().enumerate() {
                for (dy, color) in line.iter_mut().enumerate() {
                    let (x, y) = (column * PLACES.0 + dx, row * PLACES.1 + dy);
                    *color = paint(Pt {
                        x: ((x as f32 + 0.5) / across as f32 - 0.5) * aspect,
                        y: (y as f32 + 0.5) / down as f32,
                        aspect,
                        dot: (x as u32, y as u32),
                    });
                }
            }
            each(cell);
        }
    }
}

fn mean(cell: &[[Rgb; 4]; 2]) -> Rgb {
    cell.iter()
        .flatten()
        .fold(Rgb::BLACK, |sum, color| sum.add(*color))
        .scale(1.0 / 8.0)
}

fn brightness(color: Rgb) -> f32 {
    (0.5 * color.max() + 0.5 * color.luma())
        .clamp(0.0, 1.0)
        .powf(0.8)
}

/// A colour lifted so a dim one still shows its hue against the tone.
fn lifted(color: Rgb, power: f32) -> Rgb {
    color.scale((1.0 / color.max().max(0.05)).powf(power))
}

fn tone(all: Rgb, background: Rgb) -> Rgb {
    background.mix(all.scale(TONE_DEPTH), TONE)
}

pub fn braille(width: u16, height: u16, background: Rgb, paint: impl Fn(Pt) -> Rgb) -> Vec<Cell> {
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    places(width, height, paint, |cell| {
        let (mut bits, mut lit, mut count) = (0u8, Rgb::BLACK, 0.0);
        for dx in 0..PLACES.0 {
            for dy in 0..PLACES.1 {
                let threshold = (f32::from(ORDER[dx][dy]) + 0.5) / 8.0 * 0.85;
                if (brightness(cell[dx][dy]) * GAIN) > threshold {
                    bits |= BITS[dx][dy];
                    lit = lit.add(cell[dx][dy]);
                    count += 1.0;
                }
            }
        }
        cells.push(Cell {
            symbol: char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' '),
            fg: terminal(lifted(lit.scale(1.0 / f32::max(count, 1.0)), 0.4)),
            bg: terminal(tone(mean(&cell), background)),
        });
    });
    cells
}

pub fn ascii(width: u16, height: u16, background: Rgb, paint: impl Fn(Pt) -> Rgb) -> Vec<Cell> {
    let glyphs: Vec<char> = RAMP.chars().collect();
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    places(width, height, paint, |cell| {
        let all = mean(&cell);
        let at = (brightness(all) * (glyphs.len() - 1) as f32).round() as usize;
        cells.push(Cell {
            symbol: glyphs[at],
            fg: terminal(lifted(all, 0.6)),
            bg: terminal(tone(all, background)),
        });
    });
    cells
}
