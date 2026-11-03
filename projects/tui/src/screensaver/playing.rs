//! A scene playing in a window over the app.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, BorderType, Clear},
};

use super::{halftone, paint::Rgb, scenes::Scene};
use crate::ui::theme;

/// What stays of the app around the window, in cells.
const MARGIN_X: u16 = 3;
const MARGIN_Y: u16 = 1;
const FPS: u32 = 12;
/// The window's own colour, `theme::SURFACE`, between the dots.
const BACKGROUND: Rgb = Rgb::hex(0x161a23);

pub struct Playing {
    scene: Scene,
    started: Instant,
}

impl Playing {
    pub fn random() -> Option<Self> {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .subsec_nanos();
        Some(Self {
            scene: Scene::ALL[seed as usize % Scene::ALL.len()],
            started: Instant::now(),
        })
    }

    pub fn frame_period(&self) -> Duration {
        Duration::from_secs(1) / FPS
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
            .border_style(Style::new().fg(theme::HAIRLINE))
            .title(format!(" {} ", self.scene.name()))
            .title_style(Style::new().fg(theme::TEXT_MUTED))
            .style(Style::new().bg(theme::SURFACE));
        let inner = window.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(window, area);
        let t = self.started.elapsed().as_secs_f32();
        let scene = self.scene;
        let cells = halftone::render(inner.width, inner.height, BACKGROUND, |p| scene.paint(p, t));
        let buffer = frame.buffer_mut();
        for (index, cell) in cells.into_iter().enumerate() {
            let x = inner.x + (index % usize::from(inner.width)) as u16;
            let y = inner.y + (index / usize::from(inner.width)) as u16;
            if let Some(target) = buffer.cell_mut((x, y)) {
                target.set_char(cell.symbol).set_fg(cell.fg).set_bg(cell.bg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_lights_dots_at_any_size() {
        for scene in Scene::ALL {
            for (width, height) in [(1, 1), (7, 3), (120, 40)] {
                for t in [0.0, 3.7, 600.0] {
                    let cells = halftone::render(width, height, BACKGROUND, |p| scene.paint(p, t));
                    assert_eq!(cells.len(), usize::from(width) * usize::from(height));
                }
            }
            let lit = halftone::render(120, 40, BACKGROUND, |p| scene.paint(p, 2.0))
                .iter()
                .filter(|cell| cell.symbol != '\u{2800}')
                .count();
            assert!(lit > 1000, "{scene:?} lit only {lit} cells");
        }
    }

    #[test]
    fn each_scene_adds_about_ten_percent_more_dots() {
        // Lit Braille dots at 120x40, t=3.7, before the atmosphere/density change
        // (c7c325845a975b6c5f3235647fb4398cc7c4490d). These are dot counts, not
        // nonempty cells: filling an existing cell must count too.
        let baseline = [
            20_635, 23_062, 9_452, 26_602, 7_532, 20_253, 8_389, 8_021, 18_605, 12_991, 19_564,
        ];
        for (scene, before) in Scene::ALL.into_iter().zip(baseline) {
            let cells = halftone::render(120, 40, BACKGROUND, |p| scene.paint(p, 3.7));
            let after: u32 = cells
                .iter()
                .map(|cell| (cell.symbol as u32 - 0x2800).count_ones())
                .sum();
            let gain = after as f32 / before as f32;
            // Ordered dithering, new atmosphere and highlight saturation give a
            // small scene-specific spread around the requested ten percent.
            assert!(
                (1.07..=1.13).contains(&gain),
                "{scene:?}: {before} -> {after} ({gain:.3})"
            );
        }
    }
}
