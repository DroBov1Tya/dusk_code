//! The thread name sits quietly above the transcript.

use super::{
    theme::{DOT, NIGHT, SUN, SUN_GLYPH, VIOLET, bold, fg, thought},
    wrap,
};
use crate::app::App;
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let brand = vec![
        Span::styled(format!(" {SUN_GLYPH} "), bold(SUN)),
        Span::styled("dusk", bold(VIOLET)),
        Span::styled(format!("  {DOT}  "), fg(NIGHT)),
    ];
    let room = (area.width as usize)
        .saturating_sub(brand.iter().map(|span| span.content.as_ref().width()).sum());
    let title = match &app.title {
        Some(title) => Span::styled(wrap::clip(title, room), ratatui::style::Style::new()),
        None => Span::styled(wrap::clip("new thread", room), thought()),
    };
    let mut spans = brand;
    spans.push(title);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
