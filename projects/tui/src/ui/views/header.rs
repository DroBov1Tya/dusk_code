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
    // The rail shows the effort; without it, the header does.
    let effort = if app.sidebar_visible && !app.compact_layout {
        String::new()
    } else {
        format!(
            " effort {} ",
            app.effort.map_or("default", |effort| effort.name())
        )
    };
    let effort_width = effort.width().min(area.width as usize);
    let title_area = Rect {
        width: area.width.saturating_sub(effort_width as u16),
        ..area
    };
    let room = (title_area.width as usize)
        .saturating_sub(brand.iter().map(|span| span.content.as_ref().width()).sum());
    let title = match &app.title {
        Some(title) => Span::styled(wrap::clip(title, room), ratatui::style::Style::new()),
        None => Span::styled(wrap::clip("new thread", room), thought()),
    };
    let mut spans = brand;
    spans.push(title);
    frame.render_widget(Paragraph::new(Line::from(spans)), title_area);
    frame.render_widget(
        Paragraph::new(Span::styled(effort, thought())).right_aligned(),
        Rect {
            x: area.right().saturating_sub(effort_width as u16),
            width: effort_width as u16,
            ..area
        },
    );
}
