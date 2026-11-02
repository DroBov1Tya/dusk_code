//! The look: dusk, after the sun has set. The chrome is ink and three greys of text,
//! with two accents from the evening sky: amber, the last light at the horizon, where
//! something needs the eye (work going on, the choice under the cursor), and a dark
//! violet above it for names and headings. Green and red only
//! say done and failed. Colour beyond that is kept for the moments that are content:
//! the agents' figures, code, the context gauge and the idle screen. Main text keeps
//! the terminal's own colour, so light and dark terminals both read well.

use ratatui::style::{Color, Modifier, Style};

/// The rail and status bar, the deepest surface.
pub const DEEP: Color = Color::Rgb(17, 20, 27);
/// Behind the message box and windows.
pub const SURFACE: Color = Color::Rgb(22, 26, 35);
/// A step up: the browsed entry, a message card, buttons, queued messages.
pub const RAISED: Color = Color::Rgb(30, 35, 46);
/// Borders and separators, quiet hairlines.
pub const HAIRLINE: Color = Color::Rgb(46, 52, 66);
/// Text marked with the mouse.
pub const SELECTED: Color = Color::Rgb(48, 58, 80);

/// The three roles of text: what is read, what explains it, and what is said in passing.
pub const TEXT: Color = Color::Rgb(230, 232, 238);
pub const TEXT_SOFT: Color = Color::Rgb(163, 169, 184);
pub const TEXT_MUTED: Color = Color::Rgb(122, 130, 150);

/// The accent: the last light at the horizon, for what needs the eye now.
pub const ACCENT: Color = Color::Rgb(232, 168, 90);
/// The sky above it: names and headings, the modes that are not MAIN.
pub const DUSK: Color = Color::Rgb(128, 108, 204);
/// Done and failed, toned to sit with the ink.
pub const POSITIVE: Color = Color::Rgb(76, 195, 138);
pub const NEGATIVE: Color = Color::Rgb(229, 72, 77);

// Glyphs, written as escapes to keep the sources ASCII.
pub const SUN_GLYPH: &str = "\u{25D2}";
pub const FOLDED: &str = "\u{25B8}";
pub const UNFOLDED: &str = "\u{25BE}";
pub const USER_BAR: &str = "\u{258E}";
pub const GUTTER: &str = "\u{2502}";
pub const CODE_BAR: &str = "\u{258F}";
pub const TOOL: &str = "\u{25C6}";
pub const TOOL_RUNNING: &str = "\u{25C7}";
pub const DONE: &str = "\u{2713}";
pub const FAILED: &str = "\u{2717}";
pub const DOT: &str = "\u{00B7}";
pub const BULLET: &str = "\u{2022}";
pub const ELLIPSIS: &str = "\u{2026}";
pub const ON: &str = "\u{25CF}";
pub const OFF: &str = "\u{25CB}";
pub const SPINNER: [&str; 6] = [
    "\u{25C7}\u{00B7}  ",
    "\u{25C7}\u{00B7}\u{00B7} ",
    "\u{25C7}\u{00B7}\u{00B7}\u{00B7}",
    "\u{25C6}\u{00B7}\u{00B7}\u{00B7}",
    "\u{25C6}\u{00B7}\u{00B7} ",
    "\u{25C6}\u{00B7}  ",
];

pub fn fg(color: Color) -> Style {
    Style::new().fg(color)
}

pub fn dim() -> Style {
    fg(TEXT_MUTED)
}

pub fn thought() -> Style {
    fg(TEXT_MUTED).add_modifier(Modifier::ITALIC)
}

pub fn bold(color: Color) -> Style {
    fg(color).add_modifier(Modifier::BOLD)
}

/// How full something is, from quiet ink through the amber to red at the end: `at`
/// runs from 0 to 1.
pub fn dusk(at: f64) -> Color {
    const STOPS: [(f64, (u8, u8, u8)); 4] = [
        (0.0, (122, 130, 150)),
        (0.5, (163, 169, 184)),
        (0.8, (232, 168, 90)),
        (1.0, (229, 72, 77)),
    ];
    let at = at.clamp(0.0, 1.0);
    let upper = STOPS
        .iter()
        .position(|&(stop, _)| stop >= at)
        .unwrap_or(STOPS.len() - 1);
    let (end, to) = STOPS[upper.max(1)];
    let (start, from) = STOPS[upper.max(1) - 1];
    let mix = (at - start) / (end - start);
    let channel = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * mix).round() as u8;
    Color::Rgb(
        channel(from.0, to.0),
        channel(from.1, to.1),
        channel(from.2, to.2),
    )
}
