//! Typed root image results as ordinary terminal cells. No terminal graphics escapes
//! or link detection: half blocks participate in clipping, scrolling and selection.

use dusk_core::state::artifacts::ImageArtifact;
use image::{
    Rgba,
    imageops::{self, FilterType},
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::{
    theme::{HAZE, TEAL, bold, dim},
    wrap,
};
use crate::app::Transcript;

/// The preview's largest size in cells; each cell shows two pixels, one above the other.
const THUMBNAIL_WIDTH: usize = 56;
const THUMBNAIL_HEIGHT: usize = 14;
const STACK_AT: usize = 60;
const GAP: usize = 2;
const BACKGROUND: [u8; 3] = [30, 27, 42];

pub fn render(
    transcript: &Transcript,
    artifact: &ImageArtifact,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(2);
    let stacked = width < STACK_AT;
    let thumbnail_width = THUMBNAIL_WIDTH.min(if stacked { width } else { width / 2 });
    let text_width = if stacked {
        width
    } else {
        width - thumbnail_width - GAP
    };
    let title = artifact
        .title
        .as_deref()
        .map(plain)
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Image".into());
    let mut text: Vec<Line<'static>> =
        wrap::words(vec![Span::styled(title, bold(TEAL))], text_width, 0)
            .into_iter()
            .map(Line::from)
            .collect();
    if let Some(description) = &artifact.description {
        text.extend(
            wrap::words(vec![Span::raw(plain(description))], text_width, 0)
                .into_iter()
                .map(Line::from),
        );
    }
    let details = format!(
        "{}\u{00d7}{}  {}",
        artifact.width,
        artifact.height,
        plain(&artifact.mime_type)
    );
    text.extend(
        wrap::words(vec![Span::styled(details, dim())], text_width, 0)
            .into_iter()
            .map(Line::from),
    );

    let preview = match transcript.images.thumbnail(artifact) {
        Some(image) => {
            let scale = (thumbnail_width as f64 / image.width() as f64)
                .min((THUMBNAIL_HEIGHT * 2) as f64 / image.height() as f64);
            let w = ((image.width() as f64 * scale).floor() as u32).max(1);
            let h = ((image.height() as f64 * scale).floor() as u32).max(1);
            let image = imageops::resize(&*image, w, h, FilterType::Lanczos3);
            (0..h)
                .step_by(2)
                .map(|y| {
                    let mut spans = Vec::with_capacity(w as usize + 1);
                    for x in 0..w {
                        let upper = pixel(*image.get_pixel(x, y));
                        let lower = if y + 1 < h {
                            pixel(*image.get_pixel(x, y + 1))
                        } else {
                            Color::Rgb(BACKGROUND[0], BACKGROUND[1], BACKGROUND[2])
                        };
                        spans.push(Span::styled(
                            "\u{2580}",
                            Style::new()
                                .fg(upper)
                                .bg(lower)
                                .remove_modifier(Modifier::DIM | Modifier::BOLD),
                        ));
                    }
                    spans.push(Span::raw(
                        " ".repeat(thumbnail_width.saturating_sub(w as usize)),
                    ));
                    Line::from(spans)
                })
                .collect::<Vec<_>>()
        }
        None => wrap::words(
            vec![Span::styled("[image unavailable]", Style::new().fg(HAZE))],
            thumbnail_width,
            0,
        )
        .into_iter()
        .map(|spans| {
            let used: usize = spans.iter().map(Span::width).sum();
            let mut spans = spans;
            spans.push(Span::raw(" ".repeat(thumbnail_width.saturating_sub(used))));
            Line::from(spans)
        })
        .collect(),
    };
    if stacked {
        // Heading first on narrow terminals; the preview and description follow.
        let title_lines = wrap::words(
            vec![Span::styled(
                artifact
                    .title
                    .as_deref()
                    .map(plain)
                    .filter(|title| !title.is_empty())
                    .unwrap_or_else(|| "Image".into()),
                bold(TEAL),
            )],
            width,
            0,
        )
        .len();
        let mut lines = text.drain(..title_lines).collect::<Vec<_>>();
        lines.extend(preview);
        lines.extend(text);
        return lines;
    }
    (0..preview.len().max(text.len()))
        .map(|row| {
            let mut spans = preview
                .get(row)
                .map(|line| line.spans.clone())
                .unwrap_or_else(|| vec![Span::raw(" ".repeat(thumbnail_width))]);
            spans.push(Span::raw(" ".repeat(GAP)));
            if let Some(line) = text.get(row) {
                spans.extend(line.spans.clone());
            }
            Line::from(spans)
        })
        .collect()
}

fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn pixel(pixel: Rgba<u8>) -> Color {
    let alpha = pixel[3] as u16;
    let channel = |index: usize| {
        ((pixel[index] as u16 * alpha + BACKGROUND[index] as u16 * (255 - alpha)) / 255) as u8
    };
    Color::Rgb(channel(0), channel(1), channel(2))
}
