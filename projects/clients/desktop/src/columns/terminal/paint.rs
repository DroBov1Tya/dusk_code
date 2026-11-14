//! Drawing a terminal's screen: each run of cells on its background, in the code font,
//! at its column, so a line of text cannot push the cells after it out of place.

use std::{cell::Cell, rc::Rc};

use gpui_kit::{component::ActiveTheme, *};

use super::screen::{Ink, Picture, Style};

/// The size of one cell and the font it is drawn in.
#[derive(Clone)]
pub struct Metrics {
    pub cell: Size<Pixels>,
    pub font_size: Pixels,
    pub family: SharedString,
}

pub fn metrics(window: &Window, cx: &App) -> Metrics {
    let prefs = crate::ui::theme::prefs(cx);
    let family = SharedString::from(
        prefs
            .mono_font
            .clone()
            .unwrap_or_else(|| "Menlo".to_owned()),
    );
    let font_size = px(prefs.mono_size);
    let system = window.text_system();
    let id = system.resolve_font(&font(family.clone()));
    let width = system.em_advance(id, font_size).unwrap_or(font_size * 0.6);
    Metrics {
        cell: size(width, (font_size * 1.35).round()),
        font_size,
        family,
    }
}

/// The colors a program may name. The sixteen are the theme's own, so a prompt and a
/// diff look at home; the rest of the 256 are the standard cube and greys.
#[derive(Clone)]
pub struct Palette {
    pub foreground: Hsla,
    pub background: Hsla,
    pub cursor: Hsla,
    named: [Hsla; 16],
}

impl Palette {
    pub fn of(cx: &App) -> Self {
        let theme = cx.theme();
        let hex = |value: u32| -> Hsla { rgb(value).into() };
        let named = if theme.is_dark() {
            [
                hex(0x262839),
                theme.danger,
                theme.success,
                theme.warning,
                hex(0x8fa8f8),
                theme.primary,
                hex(0x7cc7e8),
                hex(0xaeadc4),
                hex(0x83829c),
                hex(0xff9bb0),
                hex(0x93e6c8),
                hex(0xf6d78e),
                hex(0xa9bcff),
                hex(0xcbbcff),
                hex(0x9fdcf4),
                theme.foreground,
            ]
        } else {
            [
                hex(0x171717),
                hex(0xc0392b),
                hex(0x1e8e4a),
                hex(0xa06a00),
                hex(0x2563c9),
                hex(0x8b3fc7),
                hex(0x0e7c86),
                hex(0x737373),
                hex(0x525252),
                hex(0xdc2626),
                hex(0x16a34a),
                hex(0xb7791f),
                hex(0x3b82f6),
                hex(0xa855f7),
                hex(0x0891b2),
                hex(0x404040),
            ]
        };
        Self {
            foreground: theme.foreground,
            background: theme.background,
            cursor: theme.primary,
            named,
        }
    }

    fn color(&self, ink: Ink) -> Hsla {
        match ink {
            Ink::Foreground => self.foreground,
            Ink::Background => self.background,
            Ink::Rgb(r, g, b) => rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)).into(),
            Ink::Indexed(index) => match index {
                0..=15 => self.named[usize::from(index)],
                // A 6x6x6 cube, then 24 greys.
                16..=231 => {
                    let cube = u32::from(index) - 16;
                    let level = |step: u32| if step == 0 { 0 } else { 55 + step * 40 };
                    rgb(level(cube / 36) << 16 | level(cube / 6 % 6) << 8 | level(cube % 6)).into()
                }
                232..=255 => {
                    let grey = 8 + (u32::from(index) - 232) * 10;
                    rgb(grey << 16 | grey << 8 | grey).into()
                }
            },
        }
    }
}

/// The screen as an element that fills its place. It leaves the size it was given in
/// `measured`, from which the panel works out how many cells fit.
pub fn grid(
    picture: Picture,
    metrics: Metrics,
    palette: Palette,
    focused: bool,
    measured: Rc<Cell<Option<Size<Pixels>>>>,
) -> impl IntoElement {
    canvas(
        move |bounds, _, _| measured.set(Some(bounds.size)),
        move |bounds, _, window, cx| {
            let cell = metrics.cell;
            let place = |row: usize, col: usize| {
                point(
                    bounds.origin.x + cell.width * col as f32,
                    bounds.origin.y + cell.height * row as f32,
                )
            };
            for (row, runs) in picture.rows.iter().enumerate() {
                for run in runs {
                    let origin = place(row, run.col);
                    if run.style.bg != Ink::Background {
                        let area = size(cell.width * run.cells as f32, cell.height);
                        window.paint_quad(fill(
                            Bounds::new(origin, area),
                            palette.color(run.style.bg),
                        ));
                    }
                    if run.text.trim().is_empty() {
                        continue;
                    }
                    let shaped = window.text_system().shape_line(
                        SharedString::from(run.text.clone()),
                        metrics.font_size,
                        &[text_run(run.text.len(), run.style, &metrics, &palette)],
                        None,
                    );
                    // A line that cannot be drawn is left blank; the next frame tries again.
                    let _ = shaped.paint(origin, cell.height, TextAlign::Left, None, window, cx);
                }
            }
            if let Some((row, col)) = picture.cursor {
                let block = Bounds::new(place(row, col), cell);
                // A block while the keys go here; a hollow one while they go elsewhere.
                window.paint_quad(match focused {
                    true => fill(block, palette.cursor.opacity(0.55)),
                    false => outline(block, palette.cursor.opacity(0.7), BorderStyle::Solid),
                });
            }
        },
    )
    .size_full()
}

fn text_run(len: usize, style: Style, metrics: &Metrics, palette: &Palette) -> TextRun {
    let mut face = font(metrics.family.clone());
    if style.bold {
        face = face.bold();
    }
    if style.italic {
        face = face.italic();
    }
    let color = palette.color(style.fg);
    TextRun {
        len,
        font: face,
        color: if style.dim { color.opacity(0.6) } else { color },
        background_color: None,
        underline: style.underline.then(|| UnderlineStyle {
            thickness: px(1.),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: None,
    }
}
