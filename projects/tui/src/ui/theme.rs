//! The look: a dusk sky. Violet at the top of the evening, rose and amber where the sun
//! goes down, ember when the context is nearly full. Main text keeps the terminal's own
//! colour, so light and dark terminals both read well.

use ratatui::style::{Color, Modifier, Style};

pub const VIOLET: Color = Color::Rgb(157, 124, 255);
pub const ROSE: Color = Color::Rgb(240, 122, 176);
pub const SUN: Color = Color::Rgb(255, 180, 84);
pub const EMBER: Color = Color::Rgb(255, 99, 99);
pub const TEAL: Color = Color::Rgb(94, 214, 194);
/// Thinking, hints and anything said in passing.
pub const HAZE: Color = Color::Rgb(128, 122, 150);
/// Quiet borders and separators.
pub const NIGHT: Color = Color::Rgb(68, 63, 88);
/// Behind the selected entry.
pub const GLOW: Color = Color::Rgb(44, 40, 60);
/// Text marked with the mouse.
pub const SELECTED: Color = Color::Rgb(78, 66, 112);
/// A quiet contrast for the full-height statistics rail.
pub const SIDEBAR_BG: Color = Color::Rgb(38, 35, 52);
/// Behind the message box, so it stands apart from the transcript.
pub const INPUT_BG: Color = Color::Rgb(30, 27, 42);
pub const SIDEBAR_FG: Color = Color::Rgb(230, 224, 240);
pub const SIDEBAR_MUTED: Color = Color::Rgb(165, 157, 187);
pub const SIDEBAR_EDGE: Color = Color::Rgb(107, 96, 132);

// Glyphs, written as escapes to keep the sources ASCII.
pub const SUN_GLYPH: &str = "\u{25D2}";
pub const FOLDED: &str = "\u{25B8}";
pub const UNFOLDED: &str = "\u{25BE}";
pub const USER_BAR: &str = "\u{258C}";
pub const GUTTER: &str = "\u{2502}";
pub const CODE_BAR: &str = "\u{258F}";
pub const HORIZON_DARK: &str = "\u{2500}";
pub const TOOL: &str = "\u{25C6}";
pub const TOOL_RUNNING: &str = "\u{25C7}";
pub const DONE: &str = "\u{2713}";
pub const FAILED: &str = "\u{2717}";
pub const DOT: &str = "\u{00B7}";
pub const BULLET: &str = "\u{2022}";
pub const ELLIPSIS: &str = "\u{2026}";
pub const ON: &str = "\u{25CF}";
/// A provider that answered, and one that did not.
pub const ALIVE: &str = "\u{1F7E2}";
pub const DOWN: &str = "\u{1F534}";
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
    fg(HAZE)
}

pub fn thought() -> Style {
    fg(HAZE).add_modifier(Modifier::ITALIC)
}

pub fn bold(color: Color) -> Style {
    fg(color).add_modifier(Modifier::BOLD)
}

/// The sky from early evening to the last light: `at` runs from 0 to 1.
pub fn dusk(at: f64) -> Color {
    const STOPS: [(f64, (u8, u8, u8)); 4] = [
        (0.0, (157, 124, 255)),
        (0.45, (240, 122, 176)),
        (0.75, (255, 180, 84)),
        (1.0, (255, 99, 99)),
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
