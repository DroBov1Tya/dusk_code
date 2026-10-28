//! The top of the screen: the name of the thread and the model, and under them the
//! horizon. The horizon is the context meter: it lights up from the left as the context
//! fills, the sun sets along it, and a mark shows where compaction begins.

use dusk_core::runtime::AUTO_COMPACT_PERCENT;
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::{
    theme::{
        COMPACT_MARK, DOT, HAZE, HORIZON_DARK, HORIZON_LIT, NIGHT, SETTING_SUN, SUN, SUN_GLYPH,
        VIOLET, bold, dim, dusk, fg, thought,
    },
    wrap,
};
use crate::app::App;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let brand = vec![
        Span::styled(format!(" {SUN_GLYPH} "), bold(SUN)),
        Span::styled("dusk", bold(VIOLET)),
        Span::styled(format!("  {DOT}  "), fg(NIGHT)),
    ];
    // On a narrow screen the thread and the model share what is left, half each.
    let available = (area.width as usize).saturating_sub(13);
    let model = format!("{}  ", wrap::clip(&app.model, available / 2));
    let room = available.saturating_sub(model.width() + 1);
    let title = match &app.title {
        Some(title) => Span::styled(wrap::clip(title, room), ratatui::style::Style::new()),
        None => Span::styled(wrap::clip("new thread", room), thought()),
    };
    let gap = room.saturating_sub(title.width()) + 1;
    let mut spans = brand;
    spans.extend([
        title,
        Span::raw(" ".repeat(gap)),
        Span::styled(model, dim()),
    ]);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn horizon(frame: &mut Frame, area: Rect, app: &App) {
    let width = area.width as usize;
    let fill = filled(app);
    let lit = (fill * width as f64).round() as usize;
    let mark = width * AUTO_COMPACT_PERCENT as usize / 100;
    let spans: Vec<Span> = (0..width)
        .map(|x| {
            let sky = dusk(x as f64 / width.max(2).saturating_sub(1) as f64);
            if lit > 0 && lit < width && x + 1 == lit {
                Span::styled(SETTING_SUN, fg(sky))
            } else if x < lit {
                Span::styled(HORIZON_LIT, fg(sky))
            } else if x == mark {
                Span::styled(COMPACT_MARK, fg(HAZE))
            } else {
                Span::styled(HORIZON_DARK, fg(NIGHT))
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// How full the context is, from 0 to 1.
pub fn filled(app: &App) -> f64 {
    if app.limit == 0 {
        return 0.0;
    }
    (app.tokens.context as f64 / app.limit as f64).clamp(0.0, 1.0)
}
