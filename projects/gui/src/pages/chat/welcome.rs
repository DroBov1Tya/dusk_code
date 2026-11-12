//! The page of a chat not started yet: a line in a warm serif, as on a cafe's board, that
//! changes with the time of day, and under it where the first message will go.

use chrono::Timelike;
use gpui_kit::{component::ActiveTheme, *};

use crate::{ui::Type, workspace::Workspace};

/// Warm serif faces, the first one installed is used; the interface font otherwise.
const FACES: &[&str] = &[
    "Fraunces",
    "Playfair Display",
    "Cormorant Garamond",
    "Iowan Old Style",
    "Palatino",
    "Baskerville",
    "Georgia",
    "DejaVu Serif",
    "Liberation Serif",
];

const MORNING: &[&str] = &[
    "Fresh page, fresh coffee. What are we making?",
    "Good morning. Let's start with the one thing that matters.",
    "The day is still quiet. A good time for a hard problem.",
    "Morning. Tell me the idea before it gets away.",
];
const DAY: &[&str] = &[
    "What shall we build today?",
    "Got an idea worth a second cup?",
    "Pull up a chair. What's on your mind?",
    "Small step or big leap, I'm in.",
    "Let's turn that thought into something that runs.",
];
const EVENING: &[&str] = &[
    "Dusk is a good hour for a new idea.",
    "The light is going soft. What are we finishing?",
    "One more good thing before the day ends?",
    "Evening. Let's make tomorrow's work easier.",
];
const NIGHT: &[&str] = &[
    "Still up? Let's make it count.",
    "The quiet hours. Nobody will interrupt us now.",
    "Night shift. I'll keep the lamp on.",
];

/// The line for this hour; it holds for the hour and differs from day to day.
fn line(now: chrono::DateTime<chrono::Local>) -> &'static str {
    let lines = match now.hour() {
        5..=10 => MORNING,
        11..=16 => DAY,
        17..=21 => EVENING,
        _ => NIGHT,
    };
    let turn = now.timestamp() / 3600;
    lines[turn.rem_euclid(lines.len() as i64) as usize]
}

pub(super) fn render(ws: &Workspace, cx: &App) -> Div {
    let theme = cx.theme();
    let hint = if ws.projects.is_empty() {
        "Add a project on the left to start.".to_owned()
    } else if let Some(project) = ws.selected_project() {
        format!("Write a message to start a chat in {}.", project.name)
    } else {
        "Write a message to start a chat, or pick one on the left.".to_owned()
    };
    let face = FACES
        .iter()
        .find(|face| ws.fonts.iter().any(|family| family == *face));
    let mut greeting = div()
        .max_w(px(620.))
        .text_center()
        .text_size(rems(2.1))
        .line_height(relative(1.25))
        .italic()
        .text_color(theme.foreground.opacity(0.92))
        .child(line(chrono::Local::now()));
    if let Some(face) = face {
        greeting = greeting.font_family(*face);
    }
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_4()
        .px_8()
        .child(greeting)
        .child(div().small().text_color(theme.muted_foreground).child(hint))
}
