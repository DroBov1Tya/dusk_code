//! Drawing the app. From the top: the thread and the model, the horizon that shows how
//! full the context is, the transcript, then the status rule and the message box.

mod header;
mod input;
mod markdown;
mod panels;
pub mod screen;
mod theme;
mod transcript;
mod wrap;

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
};

use crate::app::App;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let screen = frame.area();
    let input_height = input::height(app, screen.width);
    // The empty line under the horizon lets the transcript breathe.
    let [top, horizon, _, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(input_height),
    ])
    .areas(screen);
    header::draw(frame, top, app);
    header::horizon(frame, horizon, app);
    transcript::draw(frame, body, app);
    input::draw(frame, bottom, app);
    input::draw_commands(frame, body, app);
    panels::draw(frame, screen, app);
}
