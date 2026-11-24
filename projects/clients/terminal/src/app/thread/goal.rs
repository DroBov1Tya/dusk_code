//! The open thread's standing goal, shown above the box with a button that clears it
//! the way `/goal clear` does.

use dusk_protocol::thread::Goal;

use super::{Action, App, Tone};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GoalButton {
    Clear,
}

/// What can be done with `goal` now, by its buttons.
pub fn buttons(_goal: &Goal) -> &'static [GoalButton] {
    &[GoalButton::Clear]
}

impl GoalButton {
    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => " \u{00D7} ",
        }
    }
}

/// A goal button on the last drawn frame.
pub struct GoalHit {
    pub row: u16,
    pub left: u16,
    pub right: u16,
    pub button: GoalButton,
}

impl App {
    pub fn goal_changed(&mut self, goal: Option<Goal>, note: String, started: Option<String>) {
        // A turn the goal starts shows as the message that starts it.
        if !note.is_empty() {
            self.transcript.notice(note, Tone::Info);
        }
        if let Some(text) = started {
            self.show_sent(&text);
        }
        self.goal = goal;
    }

    pub fn press_goal(&mut self, button: GoalButton) -> Action {
        if self.goal.is_none() {
            return Action::None;
        }
        match button {
            GoalButton::Clear => Action::Send("/goal clear".into()),
        }
    }
}
