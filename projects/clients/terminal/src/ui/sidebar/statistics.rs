//! The top of the rail: the model and its mode on a card, the context gauge and the
//! activity of the turn.

use dusk_protocol::thread::{AUTO_COMPACT_PERCENT, ThreadTokens};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding, Paragraph},
};

use super::{
    Sidebar,
    section::{GAUGE, PAD, fill, row, ruled},
};
use crate::app::compact_number;
use crate::ui::{
    text::format::format_duration,
    theme::{
        ACCENT, DEEP, DUSK, HAIRLINE, NEGATIVE, RAISED, TEXT, TEXT_MUTED, TEXT_SOFT, USER_BAR,
        bold, dusk, fg,
    },
};

impl Sidebar<'_> {
    pub fn statistics(&self, frame: &mut Frame, area: Rect) {
        let width = area.width.saturating_sub(2 * PAD) as usize;
        let share = if self.app.limit == 0 {
            0.0
        } else {
            (self.app.tokens.context as f64 / self.app.limit as f64).clamp(0.0, 1.0)
        };
        let percent = share * 100.0;
        let near_compaction = percent >= AUTO_COMPACT_PERCENT as f64;
        let ThreadTokens { model_calls, .. } = self.app.tokens;
        let elapsed = self
            .app
            .elapsed_ms()
            .map(format_duration)
            .unwrap_or_else(|| "--:--".into());
        let time_label = if self.app.turn_started_at.is_some() {
            "turn"
        } else {
            "last turn"
        };
        let mut calls = match model_calls {
            1 => " \u{00B7} 1 call".to_owned(),
            calls => format!(" \u{00B7} {calls} calls"),
        };
        if let Some(cost) = self.app.tokens.cost() {
            calls.push_str(&format!(" \u{00B7} {cost}"));
        }
        let rate = self
            .app
            .approximate_tokens_per_second()
            .map(|value| format!("~{value:.1} tok/s"))
            .unwrap_or_else(|| "--".into());
        let context_color = if near_compaction {
            NEGATIVE
        } else {
            dusk(share)
        };
        let mode = match self.app.effort {
            Some(effort) => format!(
                "{} \u{00B7} effort {}",
                self.app.mode.to_lowercase(),
                effort.name()
            ),
            None => self.app.mode.to_lowercase(),
        };
        let filled = ((width as f64) * share).round() as usize;
        // The gauge warms toward its end, and a notch marks where compaction starts.
        let notch = width * AUTO_COMPACT_PERCENT as usize / 100;
        let gauge: Vec<Span> = (0..width)
            .map(|cell| {
                let color = match cell {
                    _ if cell < filled && near_compaction => NEGATIVE,
                    _ if cell < filled => dusk(cell as f64 / width.max(1) as f64),
                    _ if cell == notch => TEXT_MUTED,
                    _ => HAIRLINE,
                };
                Span::styled(GAUGE, fg(color))
            })
            .collect();
        // The model and its mode on a card, with the mode's colour down its edge.
        let edge = if self.app.mode == "MAIN" {
            ACCENT
        } else {
            DUSK
        };
        let card = |text: &str, style: Style| {
            Line::from(vec![
                Span::styled(USER_BAR, fg(edge).bg(RAISED)),
                Span::styled(
                    format!(" {}", fill(text, width.saturating_sub(2))),
                    style.bg(RAISED),
                ),
            ])
        };
        let lines = vec![
            Line::default(),
            card(&self.app.model, bold(TEXT)),
            card(&mode, fg(TEXT_SOFT)),
            Line::default(),
            ruled(
                "Context",
                Some(Span::styled(format!("{percent:.0}%"), bold(context_color))),
                width,
            ),
            Line::from(gauge),
            Line::from(Span::styled(
                format!(
                    "{} of {}{}",
                    compact_number(self.app.tokens.context),
                    compact_number(self.app.limit),
                    if near_compaction {
                        "  compacting soon"
                    } else {
                        ""
                    }
                ),
                fg(TEXT_SOFT),
            )),
            Line::default(),
            ruled("Activity", None, width),
            row(
                vec![Span::styled(format!("{time_label}{calls}"), fg(TEXT_SOFT))],
                Span::styled(elapsed, fg(ACCENT)),
                width,
            ),
            row(
                vec![Span::styled("output", fg(TEXT_SOFT))],
                Span::styled(rate, fg(TEXT_SOFT)),
                width,
            ),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::new().fg(TEXT).bg(DEEP))
                .block(Block::new().padding(Padding::horizontal(PAD))),
            area,
        );
    }
}
