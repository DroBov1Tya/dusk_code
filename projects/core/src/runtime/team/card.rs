//! The task card: a member's first message, written by the core from the task's row.
//! K1 replaces the body of `card` with the packet of section 8 (what to read first, what
//! was done before, what it feeds); its callers and its uid stay.

use crate::{
    message::plain::plain,
    state::{GoalCheck, PlanTask},
    tools::short_task_id,
};

/// The card of `task`. Every line is the lead's text made plain, so none of it can end
/// its line or open a bracket of the core's.
pub fn card(task: &PlanTask) -> String {
    let mut card = format!(
        "Task #{}: {}",
        short_task_id(&task.task_id),
        plain(&task.description)
    );
    let mut line = |label: &str, text: &str| {
        if !text.trim().is_empty() {
            card.push_str(&format!("\n{label}: {}", plain(text)));
        }
    };
    line("Accept", &task.acceptance);
    match &task.gate {
        Some(GoalCheck::Command { command }) => line("Gate", command),
        Some(GoalCheck::UserConfirmation) => line("Gate", "the user confirms it"),
        None => {}
    }
    match task.files.entries() {
        [] => line("Yours", "no file; change nothing"),
        files => line("Yours", &files.join(", ")),
    }
    line("Brief", &task.brief);
    card
}

/// The card's `request_uid`: one card for each time the task is given out, so a card
/// delivered again after a stop is stored once.
pub fn uid(task: &PlanTask) -> String {
    format!("card:{}:{}", task.task_id, task.attempts)
}
