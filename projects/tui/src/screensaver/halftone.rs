//! Turns a painted picture into terminal cells: one round dot a cell, over a deep tone of
//! the same picture. The dot's size goes with the light there: the finest dots fill the
//! dark, and they swell toward highlights and lights, so a bright spot stands out of the
//! grain around it instead of only crowding it: a dot grows with its own light and more
//! with what it has over its surroundings. A cell is looked at in 2 by 4 places, so a
//! light smaller than the cell still grows its dot.

use ratatui::style::Color;

use super::paint::{Pt, Rgb};

/// Where a cell is looked at, by column and row.
const SAMPLES: (u32, u32) = (2, 4);
/// The dots by size, each from the light it takes on.
/// Three that every monospace font draws apart: small, medium and large.
const SIZES: [(f32, char); 3] = [(0.03, '\u{00B7}'), (0.42, '\u{2022}'), (0.70, '\u{25CF}')];
/// How far around a cell its surroundings reach, in cells either way: about square.
const AROUND: (i32, i32) = (3, 1);
/// How much a place's own light counts, and how much what it has over its
/// surroundings: an even field keeps fine dots, a light in it swells.
const OWN: f32 = 0.8;
const ACCENT: f32 = 1.4;
/// How much the brightest place in a cell counts against the cell's mean, so a small
/// light keeps a dot of its size.
const PEAK: f32 = 0.35;
/// How much of the picture's own colour shows between the dots, and how dark.
const TONE: f32 = 0.35;
const TONE_DEPTH: f32 = 0.3;

pub struct Cell {
    pub symbol: char,
    pub fg: Color,
    pub bg: Color,
}

/// A frame of `width` by `height` cells, row by row, painted by `paint`.
pub fn render(width: u16, height: u16, background: Rgb, paint: impl Fn(Pt) -> Rgb) -> Vec<Cell> {
    let (w, h) = (u32::from(width), u32::from(height));
    let aspect = w as f32 * SAMPLES.0 as f32 / (h as f32 * SAMPLES.1 as f32).max(1.0);
    let scale = (h * SAMPLES.1).max(1) as f32;
    let mut looked = Vec::with_capacity((w * h) as usize);
    for row in 0..h {
        for column in 0..w {
            let (mut all, mut peak) = (Rgb::BLACK, 0.0f32);
            for dx in 0..SAMPLES.0 {
                for dy in 0..SAMPLES.1 {
                    let dot = (column * SAMPLES.0 + dx, row * SAMPLES.1 + dy);
                    let color = paint(Pt {
                        x: (dot.0 as f32 + 0.5) / scale - aspect / 2.0,
                        y: (dot.1 as f32 + 0.5) / scale,
                        aspect,
                        dot,
                    });
                    all = all.add(color);
                    peak = peak.max(brightness(color));
                }
            }
            let all = all.scale(1.0 / (SAMPLES.0 * SAMPLES.1) as f32);
            looked.push((all, brightness(all) * (1.0 - PEAK) + peak * PEAK));
        }
    }
    let light_at = |column: i32, row: i32| {
        let column = column.clamp(0, w as i32 - 1) as u32;
        let row = row.clamp(0, h as i32 - 1) as u32;
        looked[(row * w + column) as usize].1
    };
    let mut cells = Vec::with_capacity(looked.len());
    for (index, &(all, light)) in looked.iter().enumerate() {
        let (column, row) = ((index as u32 % w) as i32, (index as u32 / w) as i32);
        let mut around = 0.0;
        for dx in -AROUND.0..=AROUND.0 {
            for dy in -AROUND.1..=AROUND.1 {
                around += light_at(column + dx, row + dy);
            }
        }
        let around = around / ((2 * AROUND.0 + 1) * (2 * AROUND.1 + 1)) as f32;
        let size_light = light * OWN + (light - around).max(0.0) * ACCENT;
        let bg = background.mix(all.scale(TONE_DEPTH), TONE);
        let size = SIZES.iter().rev().find(|(from, _)| size_light >= *from);
        cells.push(Cell {
            symbol: size.map_or(' ', |&(_, symbol)| symbol),
            // The dot in the picture's own hue, a dim one lifted so colour still shows
            // in the dark.
            fg: terminal(all.scale((1.0 / all.max().max(0.04)).powf(0.6))),
            bg: terminal(bg),
        });
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
    fn dots_grow_with_the_light() {
        let symbol = |light: f32| render(4, 4, Rgb::BLACK, |_| Rgb(light, light, light))[0].symbol;
        assert_eq!(symbol(0.0), ' ');
        let sizes: Vec<char> = [0.05, 0.6, 1.0].into_iter().map(symbol).collect();
        assert_eq!(sizes, SIZES.map(|(_, symbol)| symbol));
        // One bright place in a dark sky swells its dot past the finest.
        let star = render(9, 3, Rgb::BLACK, |p| {
            if p.dot == (8, 4) {
                Rgb(1.0, 1.0, 1.0)
            } else {
                Rgb::BLACK
            }
        });
        assert_ne!(star[13].symbol, SIZES[0].1);
        assert!(render(0, 20, Rgb::BLACK, |_| Rgb::BLACK).is_empty());
        assert!(render(20, 0, Rgb::BLACK, |_| Rgb::BLACK).is_empty());
    }
}
