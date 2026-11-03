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
const TONE: f32 = 0.35;
const TONE_DEPTH: f32 = 0.3;
/// Light roughly ten percent more dots without rescaling the picture or its colours.
/// Highlights already filling a cell naturally stop at its eight dots.
const DOT_DENSITY: f32 = 1.10;

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
                    if (brightness(color) * DOT_DENSITY).min(1.0) > threshold {
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
                // Dense dots in the picture's own hue, dim ones lifted so colour still
                // shows in the dark.
                lit.scale((1.0 / lit.max().max(0.04)).powf(0.6))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn darkness_and_saturated_highlights_keep_their_limits() {
        for (color, symbol) in [(Rgb::BLACK, '\u{2800}'), (Rgb(1.0, 1.0, 1.0), '\u{28ff}')] {
            assert!(
                render(8, 8, Rgb::BLACK, |_| color)
                    .iter()
                    .all(|cell| cell.symbol == symbol)
            );
        }
        assert!(render(0, 20, Rgb::BLACK, |_| Rgb::BLACK).is_empty());
        assert!(render(20, 0, Rgb::BLACK, |_| Rgb::BLACK).is_empty());
    }

    #[test]
    fn a_round_subject_stays_round_when_the_terminal_resizes() {
        for (width, height) in [(40, 40), (120, 40), (160, 50)] {
            let radius = 0.18;
            let cells = render(width, height, Rgb::BLACK, |p| {
                if p.x.powi(2) + (p.y - 0.5).powi(2) < radius * radius {
                    Rgb(1.0, 1.0, 1.0)
                } else {
                    Rgb::BLACK
                }
            });
            let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
            for (index, cell) in cells.iter().enumerate() {
                let bits = cell.symbol as u32 - 0x2800;
                for (dx, column) in DOTS.iter().enumerate() {
                    for (dy, bit) in column.iter().enumerate() {
                        if bits & bit == 0 {
                            continue;
                        }
                        let x = (index as u32 % u32::from(width)) * 2 + dx as u32;
                        let y = (index as u32 / u32::from(width)) * 4 + dy as u32;
                        left = left.min(x);
                        right = right.max(x);
                        top = top.min(y);
                        bottom = bottom.max(y);
                    }
                }
            }
            let diameter = 2.0 * radius * f32::from(height) * 4.0;
            assert!(((right - left + 1) as f32 - diameter).abs() <= 1.0);
            assert_eq!(right - left, bottom - top);
        }
    }
}
