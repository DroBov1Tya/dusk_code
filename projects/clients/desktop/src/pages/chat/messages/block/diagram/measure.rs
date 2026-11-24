//! How wide a diagram's texts are in the fonts the window draws them in, so the boxes the
//! layout makes fit the words exactly.

use std::sync::Arc;

use dusk_protocol::thread::diagram::layout::{Estimate, Measure, TextStyle};
use gpui_kit::{component::ActiveTheme, *};

/// The theme's font at the three weights a diagram uses.
pub struct Fonts {
    system: Arc<TextSystem>,
    regular: FontId,
    medium: FontId,
    semibold: FontId,
}

impl Fonts {
    pub fn new(cx: &App) -> Self {
        let system = cx.text_system().clone();
        let family = cx.theme().font_family.clone();
        let at = |weight: f32| {
            let mut face = font(family.clone());
            face.weight = FontWeight(weight);
            system.resolve_font(&face)
        };
        Self {
            regular: at(400.),
            medium: at(500.),
            semibold: at(600.),
            system,
        }
    }
}

impl Measure for Fonts {
    fn width(&self, text: &str, style: TextStyle) -> f32 {
        let id = match style.weight() {
            600.. => self.semibold,
            500..600 => self.medium,
            _ => self.regular,
        };
        let size = px(style.size());
        text.chars()
            .map(|c| match self.system.advance(id, size, c) {
                Ok(advance) => f32::from(advance.width),
                // A letter the font lacks is drawn from a fallback: guess its width.
                Err(_) => Estimate.width(c.encode_utf8(&mut [0; 4]), style),
            })
            .sum()
    }
}
