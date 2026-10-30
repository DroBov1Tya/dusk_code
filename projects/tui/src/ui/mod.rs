//! Drawing the app. The statistics rail fills the left edge on a colour of its own; the
//! thread and the message card take the rest, with air between them and no rule.

mod chat;
mod loader;
mod sidebar;
mod text;
mod theme;
mod windows;

use chat::{header, input, transcript};
use windows::panels;

use ratatui::{
    Frame,
    buffer::CellDiffOption,
    layout::{Constraint, Layout, Rect},
    style::Color,
};

use crate::app::App;

const SPLIT_AT: u16 = 84;
const SIDEBAR_WIDTH: u16 = 30;
/// Columns of air between the rail and the thread; the rail's own colour divides them.
const GAP: u16 = 2;

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
            x: screen.x + SIDEBAR_WIDTH + GAP,
            width: screen.width - SIDEBAR_WIDTH - GAP,
            ..screen
        }
    } else {
        screen
    };
    // The card reaches back to the rail, so no strip of the terminal shows beside it.
    let card = if sidebar_visible {
        Rect {
            x: main.x - GAP,
            width: main.width + GAP,
            ..main
        }
    } else {
        main
    };
    let input_height = input::height(app, card.width);
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
    input::draw(
        frame,
        Rect {
            x: card.x,
            width: card.width,
            ..bottom
        },
        app,
    );
    input::draw_commands(frame, body, app);
    input::draw_paths(frame, body, app);
    panels::draw(frame, screen, app);
    pictures(frame, app);
}

/// Pictures over the previews, unless something is drawn on top of the thread: the
/// terminal paints them above every cell, so a window would be hidden under them.
fn pictures(frame: &mut Frame, app: &App) {
    let covered = app.panel.is_some()
        || app.sidebar_overlay.is_some()
        || app.command_popup.is_some()
        || app.paths.is_some();
    if covered {
        return;
    }
    let image_fg = if frame.count() % 2 == 0 {
        Color::Reset
    } else {
        Color::Black
    };
    for shown in &app.previews {
        let artifact = &shown.artifact;
        app.pictures
            .draw(frame, shown.area, shown.card, shown.skip, artifact, || {
                app.transcript.images.original(artifact)
            });
        // Reissue the iTerm payload on each frame: equal ForcedWidth cells are skipped.
        // Its foreground does not colour the image; keep its width and covered cells.
        if let Some(cell) = frame.buffer_mut().cell_mut(shown.area)
            && matches!(cell.diff_option, CellDiffOption::ForcedWidth(width) if width.get() == 1)
            && cell.symbol().contains("\x1b]1337;File=")
        {
            cell.set_fg(image_fg);
        }
    }
}
