//! Turns a painted picture into terminal cells with block glyphs: each cell is looked at
//! in 4 by 8 places, and the glyph and two colours that draw them with the least error
//! win. The glyphs split a cell in quarters, or at any eighth of its height, or blend
//! its two colours as a light, medium or dense shade, so edges land where they are and
//! a fine texture keeps its tone.

use super::{
    cell::{Cell, terminal},
    paint::{Pt, Rgb},
};

/// Places looked at in a cell, across and down.
const ACROSS: usize = 4;
const DOWN: usize = 8;
const PLACES: usize = ACROSS * DOWN;
/// The quadrant glyph for each set of quarters drawn in the foreground colour; bit 0 is
/// the upper left, 1 the upper right, 2 the lower left, 3 the lower right.
const QUADRANTS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];
/// Shades by how much of the cell the foreground colour covers.
const SHADES: [(char, f32); 3] = [('░', 0.25), ('▒', 0.5), ('▓', 0.75)];
/// What each kind of glyph costs against its error, so the plain half block wins unless
/// another fits clearly better and a slow gradient does not flicker between glyphs.
const HALF_COST: f32 = 0.6;
const QUADRANT_COST: f32 = 1.0;
const EIGHTH_COST: f32 = 1.1;
const SHADE_COST: f32 = 1.4;
/// How far apart two channels may be, 0..1, for a cell to count as one colour.
const SAME: f32 = 0.03;

/// A way to split the places of a cell in two: those set in `mask` are the foreground.
struct Split {
    mask: u32,
    symbol: char,
    cost: f32,
}

fn splits() -> Vec<Split> {
    let mut all = Vec::new();
    let mask_of = |inside: &dyn Fn(usize, usize) -> bool| {
        (0..PLACES)
            .filter(|i| inside(i % ACROSS, i / ACROSS))
            .fold(0u32, |mask, i| mask | 1 << i)
    };
    // A mask and its complement are the same split, so the first seven cover them all.
    for (quarters, &symbol) in QUADRANTS.iter().enumerate().take(8).skip(1) {
        let mask = mask_of(&|x, y| quarters >> (x / 2 + 2 * (y / 4)) & 1 == 1);
        let cost = if quarters == 0b0011 {
            HALF_COST
        } else {
            QUADRANT_COST
        };
        all.push(Split { mask, symbol, cost });
    }
    // The lower `filled` eighths of the cell, other than the half the quarters have.
    for filled in [1, 2, 3, 5, 6, 7] {
        all.push(Split {
            mask: mask_of(&|_, y| y >= DOWN - filled),
            symbol: char::from_u32(0x2580 + filled as u32).unwrap_or(' '),
            cost: EIGHTH_COST,
        });
    }
    // The left quarter and the left three quarters; the half is a quadrant pair.
    for (columns, symbol) in [(1, '▎'), (3, '▊')] {
        all.push(Split {
            mask: mask_of(&|x, _| x < columns),
            symbol,
            cost: EIGHTH_COST,
        });
    }
    all
}

/// A frame of `width` by `height` cells, row by row, painted by `paint`.
pub fn render(width: u16, height: u16, paint: impl Fn(Pt) -> Rgb) -> Vec<Cell> {
    let (w, h) = (usize::from(width), usize::from(height));
    // Cells are twice as tall as wide, so a picture as tall as the window is `aspect` wide.
    let aspect = w as f32 / (2.0 * h.max(1) as f32);
    let (across, down) = (w * ACROSS, h * DOWN);
    let splits = splits();
    let mut cells = Vec::with_capacity(w * h);
    for row in 0..h {
        for column in 0..w {
            let mut places = [Rgb::BLACK; PLACES];
            for (i, color) in places.iter_mut().enumerate() {
                let (x, y) = (column * ACROSS + i % ACROSS, row * DOWN + i / ACROSS);
                *color = paint(Pt {
                    x: ((x as f32 + 0.5) / across as f32 - 0.5) * aspect,
                    y: (y as f32 + 0.5) / down as f32,
                    aspect,
                    // A star is a quadrant pixel, so it lands whole on a quarter glyph.
                    dot: ((x / 2) as u32, (y / 4) as u32),
                });
            }
            cells.push(fit(&places, &splits));
        }
    }
    cells
}

/// Sums over some of a cell's places: their count, colours and squared lengths.
#[derive(Default, Clone, Copy)]
struct Sum {
    count: f32,
    color: [f32; 3],
    squares: f32,
}

impl Sum {
    fn add(&mut self, c: Rgb) {
        self.count += 1.0;
        self.color[0] += c.0;
        self.color[1] += c.1;
        self.color[2] += c.2;
        self.squares += c.0 * c.0 + c.1 * c.1 + c.2 * c.2;
    }

    fn mean(self) -> Rgb {
        let k = 1.0 / self.count.max(1.0);
        Rgb(self.color[0] * k, self.color[1] * k, self.color[2] * k)
    }

    /// How far the places lie from their mean, squared and summed.
    fn error(self) -> f32 {
        let [r, g, b] = self.color;
        self.squares - (r * r + g * g + b * b) / self.count.max(1.0)
    }
}

fn fit(places: &[Rgb; PLACES], splits: &[Split]) -> Cell {
    let mut best: Option<(f32, Cell)> = None;
    let mut offer = |cost: f32, symbol: char, fg: Rgb, bg: Rgb| {
        if best.as_ref().is_none_or(|(least, _)| cost < *least) {
            let cell = if close(fg, bg) {
                let flat = terminal(fg.mix(bg, 0.5));
                Cell {
                    symbol: ' ',
                    fg: flat,
                    bg: flat,
                }
            } else {
                Cell {
                    symbol,
                    fg: terminal(fg),
                    bg: terminal(bg),
                }
            };
            best = Some((cost, cell));
        }
    };
    for split in splits {
        let (mut fg, mut bg) = (Sum::default(), Sum::default());
        for (i, color) in places.iter().enumerate() {
            if split.mask >> i & 1 == 1 {
                fg.add(*color);
            } else {
                bg.add(*color);
            }
        }
        offer(
            (fg.error() + bg.error()) * split.cost,
            split.symbol,
            fg.mean(),
            bg.mean(),
        );
    }
    // A shade blends the lighter and darker places of the cell, which suits grain.
    let light = places.iter().map(|c| brightness(*c)).sum::<f32>() / PLACES as f32;
    let (mut lighter, mut darker) = (Sum::default(), Sum::default());
    for color in places {
        if brightness(*color) >= light {
            lighter.add(*color);
        } else {
            darker.add(*color);
        }
    }
    let (fg, bg) = (lighter.mean(), darker.mean());
    for (symbol, share) in SHADES {
        let seen = bg.mix(fg, share);
        let error: f32 = places
            .iter()
            .map(|c| (c.0 - seen.0).powi(2) + (c.1 - seen.1).powi(2) + (c.2 - seen.2).powi(2))
            .sum();
        offer(error * SHADE_COST, symbol, fg, bg);
    }
    best.map_or_else(
        || Cell {
            symbol: ' ',
            fg: terminal(Rgb::BLACK),
            bg: terminal(Rgb::BLACK),
        },
        |(_, cell)| cell,
    )
}

fn close(a: Rgb, b: Rgb) -> bool {
    (a.0 - b.0)
        .abs()
        .max((a.1 - b.1).abs())
        .max((a.2 - b.2).abs())
        < SAME
}

/// How much a colour lights a place: saturated colours count as bright as they look.
fn brightness(color: Rgb) -> f32 {
    0.5 * color.max() + 0.5 * color.luma()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_split_picture_keeps_its_two_colours() {
        let (dark, light) = (Rgb(0.1, 0.1, 0.1), Rgb(0.9, 0.9, 0.9));
        let cells = render(4, 4, |p| if p.x < 0.0 { dark } else { light });
        // Left of the middle: every cell is one colour, or a vertical split at the middle.
        assert_eq!(cells[0].fg, cells[0].bg);
        assert_eq!(cells[3].fg, cells[3].bg);
        assert_ne!(cells[0].fg, cells[3].fg);
        assert!(render(0, 20, |_| dark).is_empty());
        assert!(render(20, 0, |_| dark).is_empty());
    }
}
