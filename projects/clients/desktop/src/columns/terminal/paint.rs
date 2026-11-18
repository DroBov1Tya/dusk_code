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

/// The colors a program may name. The sixteen are the theme's (`ui::theme::terminal`);
/// the rest of the 256 are the standard cube and greys.
#[derive(Clone)]
pub struct Palette {
    pub foreground: Hsla,
    pub background: Hsla,
    pub cursor: Hsla,
    pub selection: Hsla,
    named: [Hsla; 16],
}

impl Palette {
    pub fn of(cx: &App) -> Self {
        let theme = cx.theme();
        let (named, selection) = crate::ui::theme::terminal(cx);
        Self {
            foreground: theme.foreground,
            background: theme.background,
            cursor: theme.primary,
            selection,
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

/// The screen as an element that fills its place. It leaves where it was drawn in
/// `measured`, from which the panel works out how many cells fit and which one the
/// pointer is on.
pub fn grid(
    picture: Picture,
    metrics: Metrics,
    palette: Palette,
    focused: bool,
    measured: Rc<Cell<Option<Bounds<Pixels>>>>,
) -> impl IntoElement {
    canvas(
        move |bounds, _, _| measured.set(Some(bounds)),
        move |bounds, _, window, cx| {
            let cell = metrics.cell;
            let place = |row: usize, col: usize| {
                point(
                    bounds.origin.x + cell.width * col as f32,
                    bounds.origin.y + cell.height * row as f32,
                )
            };
            for (row, runs) in picture.rows.iter().enumerate() {
                for run in runs.iter().filter(|run| run.style.bg != Ink::Background) {
                    let area = size(cell.width * run.cells as f32, cell.height);
                    window.paint_quad(fill(
                        Bounds::new(place(row, run.col), area),
                        palette.color(run.style.bg),
                    ));
                }
            }
            // The selection lies over the backgrounds and under the letters.
            for (row, selected) in picture.selected.iter().enumerate() {
                if let Some((first, last)) = *selected {
                    let area = size(
                        cell.width * (last + 1).saturating_sub(first) as f32,
                        cell.height,
                    );
                    window.paint_quad(fill(
                        Bounds::new(place(row, first), area),
                        palette.selection,
                    ));
                }
            }
            for (row, runs) in picture.rows.iter().enumerate() {
                for run in runs {
                    let origin = place(row, run.col);
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
