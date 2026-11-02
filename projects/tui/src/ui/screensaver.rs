//! The idle screen: after a while with no input and no work (`appearance` in the config,
//! five minutes by default), a random animated piece
//! plays in a window over the app, a margin of the app still showing around it. Any
//! key or click puts the app back.
//!
//! Frames are rendered ahead of time by `scripts/screensaver.mjs`; see `assets/NOTICE.md`
//! for where the pieces come from. Each piece is a u32 LE length and a raw-deflated
//! block: "name\tcols\trows\tfps", a palette line "key=#rrggbb ...", then each frame as
//! `rows` lines of characters and `rows` lines of color keys.

use std::{
    collections::HashMap,
    io::Read,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use flate2::read::DeflateDecoder;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, BorderType, Clear},
};

use super::theme;

const PIECES: &[u8] = include_bytes!("../../assets/screensaver.bin");
/// What stays of the app around the window, in cells.
const MARGIN_X: u16 = 3;
const MARGIN_Y: u16 = 1;
/// A broken or hostile asset must not take the memory.
const MAX_PIECE_BYTES: u64 = 16 * 1024 * 1024;

pub struct Playing {
    name: String,
    fps: u32,
    palette: HashMap<char, Color>,
    /// Each frame's character rows and color rows.
    frames: Vec<(Vec<String>, Vec<String>)>,
    started: Instant,
}

impl Playing {
    /// A random piece, or none when no frames were built in.
    pub fn random() -> Option<Self> {
        let pieces = index(PIECES);
        if pieces.is_empty() {
            return None;
        }
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .subsec_nanos();
        let start = seed as usize % pieces.len();
        // A piece that does not read is skipped for the next one.
        (0..pieces.len()).find_map(|step| parse(pieces[(start + step) % pieces.len()]))
    }

    pub fn frame_period(&self) -> Duration {
        Duration::from_secs(1) / self.fps.max(1)
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
            .title(format!(" {} ", self.name))
            .title_style(Style::new().fg(theme::HAZE))
            .style(Style::new().bg(theme::INPUT_BG));
        let inner = window.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(window, area);
        let index = (self.started.elapsed().as_secs_f64() * f64::from(self.fps)) as usize;
        let Some((text, colors)) = self.frames.get(index % self.frames.len().max(1)) else {
            return;
        };
        let rows = text.len() as u16;
        let cols = text
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(0) as u16;
        // Centered; a piece larger than the window shows its middle.
        let (left, skip_x) = fit(inner.x, inner.width, cols);
        let (top, skip_y) = fit(inner.y, inner.height, rows);
        let buffer = frame.buffer_mut();
        for (row, (line, keys)) in text.iter().zip(colors).enumerate().skip(skip_y as usize) {
            let y = top + (row as u16 - skip_y);
            if y >= inner.bottom() {
                break;
            }
            let mut keys = keys.chars().skip(skip_x as usize);
            for (column, symbol) in line.chars().skip(skip_x as usize).enumerate() {
                let x = left + column as u16;
                if x >= inner.right() {
                    break;
                }
                let key = keys.next().unwrap_or(' ');
                let color = self
                    .palette
                    .get(&key)
                    .copied()
                    .unwrap_or(theme::SIDEBAR_MUTED);
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_char(symbol).set_fg(color).set_bg(theme::INPUT_BG);
                }
            }
        }
    }
}

/// Where content of `size` starts in a span, and how much of it is cut on each side.
fn fit(start: u16, span: u16, size: u16) -> (u16, u16) {
    if size <= span {
        (start + (span - size) / 2, 0)
    } else {
        (start, (size - span) / 2)
    }
}

fn index(mut bytes: &[u8]) -> Vec<&[u8]> {
    let mut pieces = Vec::new();
    while let Some((length, rest)) = bytes.split_first_chunk::<4>() {
        let length = u32::from_le_bytes(*length) as usize;
        let Some((piece, rest)) = rest.split_at_checked(length) else {
            break;
        };
        pieces.push(piece);
        bytes = rest;
    }
    pieces
}

fn parse(packed: &[u8]) -> Option<Playing> {
    let mut text = String::new();
    DeflateDecoder::new(packed)
        .take(MAX_PIECE_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    let mut lines = text.lines();
    let mut header = lines.next()?.split('\t');
    let name = header.next()?.to_owned();
    let (_cols, rows, fps): (u16, usize, u32) = (
        header.next()?.parse().ok()?,
        header.next()?.parse().ok()?,
        header.next()?.parse().ok()?,
    );
    let palette = lines
        .next()?
        .split_whitespace()
        .filter_map(|entry| {
            let (key, hex) = entry.split_once('=')?;
            Some((key.chars().next()?, hex_color(hex)?))
        })
        .collect();
    let lines: Vec<String> = lines.map(str::to_owned).collect();
    let frames: Vec<_> = lines
        .chunks_exact(2 * rows.max(1))
        .map(|chunk| (chunk[..rows].to_vec(), chunk[rows..].to_vec()))
        .collect();
    (!frames.is_empty()).then(|| Playing {
        name,
        fps,
        palette,
        frames,
        started: Instant::now(),
    })
}

fn hex_color(hex: &str) -> Option<Color> {
    let hex = hex.strip_prefix('#')?;
    let value = u32::from_str_radix(hex.get(..6)?, 16).ok()?;
    Some(Color::Rgb(
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
    ))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::DeflateEncoder};

    use super::*;

    #[test]
    fn a_built_piece_reads_back() {
        let block = "Sea\t2\t1\t10\na=#102030\nab\na \n~~\n  \n";
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(block.as_bytes()).unwrap();
        let packed = encoder.finish().unwrap();
        let mut bytes = (packed.len() as u32).to_le_bytes().to_vec();
        bytes.extend(&packed);
        // A cut-off trailer is ignored, not misread.
        bytes.extend([9, 0]);
        let pieces = index(&bytes);
        assert_eq!(pieces.len(), 1);
        let piece = parse(pieces[0]).unwrap();
        assert_eq!(piece.frames.len(), 2);
        assert_eq!(piece.palette[&'a'], Color::Rgb(0x10, 0x20, 0x30));
        assert_eq!(piece.frames[1].0, ["~~"]);
        assert!(parse(b"not deflate").is_none());
    }
}
