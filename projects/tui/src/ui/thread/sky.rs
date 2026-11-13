//! The sky over the thread: a few stars on the header's rows. They stand still while
//! nothing happens and twinkle while a turn runs, when now and then one falls. Only
//! empty cells are written, in colours quiet enough for a light terminal too.

use crate::app::App;
use ratatui::{Frame, layout::Rect, style::Color};

/// One cell in this many holds a star.
const SPARSE: u64 = 9;
/// Frames between two falling stars, and how many one takes to cross.
const FALL_EVERY: usize = 120;
const FALL_FRAMES: usize = 6;
/// Columns a falling star crosses, and the cells of light behind its head.
const FALL_RUN: f64 = 20.0;
const FALL_TAIL: usize = 7;

/// From the faintest star to the brightest.
const SHADES: [Color; 4] = [
    Color::Rgb(52, 56, 78),
    Color::Rgb(84, 88, 120),
    Color::Rgb(128, 124, 172),
    Color::Rgb(186, 178, 228),
];
const STARS: [&str; 4] = ["\u{00B7}", "\u{02D9}", "\u{2219}", "\u{22C6}"];
/// A stroke at the top, the middle and the foot of a cell: three steps down in one row.
const STROKES: [&str; 3] = ["\u{203E}", "\u{2500}", "_"];

/// A number that looks random and is always the same for the same cell.
pub fn scatter(x: u64, y: u64) -> u64 {
    let mut n = x
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(y.wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
    n ^= n >> 29;
    n = n.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    n ^ (n >> 32)
}

/// Stars on the empty cells of `area`, one in `sparse`; `frame` makes them twinkle.
pub fn stars(frame: &mut Frame, area: Rect, sparse: u64, turn: Option<usize>) {
    let buffer = frame.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let seed = scatter(x.into(), y.into());
            if !seed.is_multiple_of(sparse) {
                continue;
            }
            // Air on both sides, so a star never sits between two words.
            let clear = (x.saturating_sub(2)..=x + 2).all(|near| {
                buffer
                    .cell((near, y))
                    .is_none_or(|cell| cell.symbol() == " ")
            });
            if !clear {
                continue;
            }
            let Some(cell) = buffer.cell_mut((x, y)) else {
                continue;
            };
            let kind = (seed >> 8) as usize % 16;
            // Most stars are faint dots; a few are larger and brighter.
            let (glyph, base) = match kind {
                0 => (STARS[3], 2),
                1..=3 => (STARS[2], 1),
                4..=8 => (STARS[1], 1),
                _ => (STARS[0], 0),
            };
            let shade = match turn {
                Some(turn) => {
                    let phase = (seed >> 16) as f64 % std::f64::consts::TAU;
                    let pace = 0.05 + ((seed >> 24) % 5) as f64 * 0.012;
                    let wave = (turn as f64 * pace + phase).sin();
                    (base as f64 + wave * 1.2).round().clamp(0.0, 3.0) as usize
                }
                None => base,
            };
            cell.set_symbol(glyph).set_fg(SHADES[shade]);
        }
    }
}

/// The header's sky: the stars, and while a turn runs a star that falls across them.
pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    if area.width < 24 || area.height == 0 {
        return;
    }
    let at_work = app.is_busy() || app.agents_at_work();
    stars(frame, area, SPARSE, at_work.then_some(app.frame));
    if !at_work || app.frame % FALL_EVERY >= FALL_FRAMES {
        return;
    }
    let fall = (app.frame / FALL_EVERY) as u64;
    let flown = (app.frame % FALL_EVERY) as f64 / (FALL_FRAMES - 1) as f64;
    let seed = scatter(fall, 7);
    let leftward = !seed.is_multiple_of(3);
    let room = (area.width as f64 - FALL_RUN - 2.0).max(1.0);
    let start =
        area.x as f64 + 1.0 + (seed >> 8) as f64 % room + if leftward { FALL_RUN } else { 0.0 };
    let steps = (area.height as usize * STROKES.len()) as f64;
    let buffer = frame.buffer_mut();
    for behind in 0..FALL_TAIL {
        let along = flown * FALL_RUN - behind as f64;
        if along < 0.0 || along >= FALL_RUN {
            continue;
        }
        let step = (along / FALL_RUN * steps) as usize;
        let x = if leftward {
            start - along
        } else {
            start + along
        };
        let y = area.y + (step / STROKES.len()) as u16;
        let Some(cell) = buffer.cell_mut((x as u16, y)) else {
            continue;
        };
        // The head is white; the light behind it cools to the sky's violet and goes out.
        let fade = 1.0 - behind as f64 / FALL_TAIL as f64;
        let mix = |from: u8, to: u8| (from as f64 + (to as f64 - from as f64) * fade) as u8;
        cell.set_symbol(STROKES[step % STROKES.len()])
            .set_fg(Color::Rgb(mix(60, 244), mix(58, 242), mix(104, 252)));
    }
}
