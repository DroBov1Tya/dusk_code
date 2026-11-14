//! A scene playing in a window over the app.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    widgets::{Block, BorderType, Clear},
};

use super::scenes::Scene;
use crate::ui::theme;

/// What stays of the app around the window, in cells.
const MARGIN_X: u16 = 3;
const MARGIN_Y: u16 = 1;
const FPS: u32 = 12;

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
        let cells = scene
            .look()
            .render(inner.width, inner.height, |p| scene.paint(p, t));
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
    fn every_scene_fills_any_size() {
        for scene in Scene::ALL {
            for (width, height) in [(1, 1), (7, 3), (120, 40)] {
                for t in [0.0, 3.7, 600.0] {
                    let cells = scene.look().render(width, height, |p| scene.paint(p, t));
                    assert_eq!(cells.len(), usize::from(width) * usize::from(height));
                }
            }
        }
    }
}
