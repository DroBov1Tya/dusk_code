//! The cell a picture is turned into, and the ways of turning it.

use ratatui::style::Color;

use super::{
    blocks,
    paint::{Pt, Rgb},
    texture,
};

pub struct Cell {
    pub symbol: char,
    pub fg: Color,
    pub bg: Color,
}

/// The window's own colour, `theme::SURFACE`, between the dots.
const BACKGROUND: Rgb = Rgb::hex(0x161a23);

/// How a scene is drawn in cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// Quadrant blocks, four pixels a cell: whole, smooth pictures.
    Blocks,
    /// Braille dots: a fine grain, good for dark pictures and small lights.
    Braille,
    /// Glyphs of rising weight: a printed, graphic grain.
    Ascii,
}

impl Look {
    /// A frame of `width` by `height` cells, row by row, painted by `paint`.
    pub fn render(self, width: u16, height: u16, paint: impl Fn(Pt) -> Rgb) -> Vec<Cell> {
        match self {
            Self::Blocks => blocks::render(width, height, paint),
            Self::Braille => texture::braille(width, height, BACKGROUND, paint),
            Self::Ascii => texture::ascii(width, height, BACKGROUND, paint),
        }
    }
}

pub fn terminal(color: Rgb) -> Color {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::Rgb(channel(color.0), channel(color.1), channel(color.2))
}
