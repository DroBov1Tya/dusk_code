//! A pasted text open for editing.

use ratatui::{
    Frame,
    layout::{Position, Rect},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use super::frame::window;
use crate::app::{App, EditorButton, EditorHit, EditorTarget, Panel, paste};
use crate::ui::windows::{
    input,
    theme::{ACCENT, DUSK, HAIRLINE, NEGATIVE, RAISED, TEXT_SOFT, bold, fg},
};

/// A pasted text in a window over most of the screen, with its buttons on the last row.
pub fn draw_paste(frame: &mut Frame, screen: Rect, app: &mut App) {
    let cursor_allowed = app.screensaver.is_none();
    let Some(Panel::Paste(editor)) = &mut app.panel else {
        return;
    };
    let area = window(screen, 130);
    let name = match &editor.target {
        EditorTarget::Plan => "pinned plan".to_owned(),
        EditorTarget::Me => "@me".to_owned(),
        EditorTarget::Snippet(name) => format!("@{name}"),
        EditorTarget::Paste(_) => paste::label(&editor.text.text),
    };
    let mut title = vec![Span::styled(format!(" {name} "), bold(DUSK))];
    if editor.changed() {
        title.push(Span::styled("changed ", fg(ACCENT)));
    }
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(HAIRLINE))
        .title(Line::from(title));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    if inner.height < 3 {
        return;
    }
    let text_area = Rect {
        height: inner.height - 2,
        ..inner
    };
    let (rows, (column, row)) = input::layout(&editor.text, text_area.width as usize);
    let height = text_area.height as usize;
    if row < editor.scroll {
        editor.scroll = row;
    } else if row >= editor.scroll + height {
        editor.scroll = row + 1 - height;
    }
    let lines: Vec<Line> = rows
        .into_iter()
        .skip(editor.scroll)
        .take(height)
        .map(|row| Line::from(row.spans))
        .collect();
    frame.render_widget(Paragraph::new(lines), text_area);
    let bar = inner.bottom() - 1;
    let buttons = if editor.confirming {
        let question = Span::styled(" Save the changes? ", bold(ACCENT));
        let line = Rect::new(inner.x, bar, inner.width, 1);
        frame.render_widget(Paragraph::new(Line::from(question)), line);
        vec![
            (EditorButton::Save, " save  y ", ACCENT),
            (EditorButton::Discard, " discard  n ", NEGATIVE),
            (EditorButton::KeepEditing, " keep editing  Esc ", TEXT_SOFT),
        ]
    } else {
        let y = text_area.y + (row - editor.scroll) as u16;
        if cursor_allowed {
            frame.set_cursor_position(Position::new(text_area.x + column as u16, y));
        }
        vec![
            (EditorButton::Save, " save  Ctrl+S ", ACCENT),
            (EditorButton::Close, " close  Esc ", TEXT_SOFT),
        ]
    };
    let mut right = inner.right();
    for &(button, label, color) in buttons.iter().rev() {
        let width = label.width() as u16;
        let Some(left) = right.checked_sub(width + 1).filter(|&left| left >= inner.x) else {
            break;
        };
        let shown = Line::from(Span::styled(label, fg(color).bg(RAISED)));
        frame.render_widget(Paragraph::new(shown), Rect::new(left, bar, width, 1));
        app.editor_hits.push(EditorHit {
            row: bar,
            left,
            right: left + width,
            button,
        });
        right = left;
    }
}
