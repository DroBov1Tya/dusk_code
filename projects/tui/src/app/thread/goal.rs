//! The open thread's standing goal, shown above the box with buttons that send the
//! same `/goal` commands the user can type.

use dusk_core::state::{Goal, GoalStatus};

use super::{Action, App, Tone};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GoalButton {
    /// Pauses an active goal, resumes a paused one.
    Toggle,
    Clear,
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
        // A round shows as the message that starts it; the rest is worth a line.
        match started {
            Some(text) if note.starts_with("goal round") => self.show_sent(&text),
            Some(text) => {
                self.transcript.notice(note, Tone::Info);
                self.show_sent(&text);
            }
            None => self.transcript.notice(note, Tone::Info),
        }
        self.goal = goal;
    }

    pub fn press_goal(&mut self, button: GoalButton) -> Action {
        let Some(goal) = &self.goal else {
            return Action::None;
        };
        let command = match (button, goal.status) {
            (GoalButton::Clear, _) => "/goal clear",
            (GoalButton::Toggle, GoalStatus::Active) => "/goal pause",
            (GoalButton::Toggle, GoalStatus::Paused | GoalStatus::Done) => "/goal resume",
        };
        Action::Send(command.to_owned())
    }
}
