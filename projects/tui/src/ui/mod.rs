//! Drawing the app. The statistics rail fills the left edge; the thread and
//! message box occupy the right side at wider terminal widths.

mod loader;
pub mod screen;
mod text;
mod theme;
mod views;

use views::{header, input, panels, sidebar, transcript};

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    widgets::{Block, Borders},
};

use crate::app::App;

const SPLIT_AT: u16 = 84;
const SIDEBAR_WIDTH: u16 = 28;

/// Braille dots that turn in the tab title while the agent works, as the tab is all a
/// user sees of a terminal in the background.
const TITLE_SPINNER: [&str; 10] = [
    "\u{280B}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283C}", "\u{2834}", "\u{2826}", "\u{2827}",
    "\u{2807}", "\u{280F}",
];
const TITLE_IDLE: &str = "\u{2733}";

/// The terminal tab's title: a mark that turns while work goes on, then the thread.
pub fn tab_title(app: &App) -> String {
    let mark = if app.is_busy() || app.agents_at_work() {
        TITLE_SPINNER[(app.frame / 2) % TITLE_SPINNER.len()]
    } else {
        TITLE_IDLE
    };
    format!("{mark} {}", app.title.as_deref().unwrap_or("duskcode"))
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let screen = frame.area();
    app.compact_layout = screen.width < SPLIT_AT;
    let sidebar_visible = !app.compact_layout && app.sidebar_visible;
    let main = if sidebar_visible {
        Rect {
            x: screen.x + SIDEBAR_WIDTH + 1,
            width: screen.width - SIDEBAR_WIDTH - 1,
            ..screen
        }
    } else {
        screen
    };
    let input_height = input::height(app, main.width);
    // A single empty row keeps the header separate from the transcript.
    let [top, _, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(input_height),
    ])
    .areas(main);
    header::draw(frame, top, app);
    app.sidebar_overlay = None;
    app.sidebar_area = None;
    app.agent_area = None;
    app.mcp_area = None;
    app.task_hits.clear();
    app.agent_max_scroll = 0;
    app.mcp_max_scroll = 0;
    if sidebar_visible {
        let sidebar_area = Rect {
            width: SIDEBAR_WIDTH,
            ..screen
        };
        app.sidebar_area = Some(sidebar_area);
        sidebar::draw(frame, sidebar_area, app, false);
    }
    transcript::draw(frame, body, app);
    if app.compact_layout && app.sidebar_overlay_open {
        let overlay = Rect {
            width: SIDEBAR_WIDTH.min(body.width.saturating_sub(2)),
            ..body
        };
        app.sidebar_overlay = Some(overlay);
        app.sidebar_area = Some(overlay);
        sidebar::draw(frame, overlay, app, true);
    }
    input::draw(frame, bottom, app);
    input::draw_commands(frame, body, app);
    input::draw_paths(frame, body, app);
    if sidebar_visible {
        frame.render_widget(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(theme::fg(theme::SIDEBAR_EDGE)),
            Rect {
                x: screen.x + SIDEBAR_WIDTH,
                width: 1,
                ..screen
            },
        );
    }
    panels::draw(frame, screen, app);
}
