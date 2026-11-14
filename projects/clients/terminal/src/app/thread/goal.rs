//! The open thread's standing goal, shown above the box with buttons that send the
//! same `/goal` commands the user can type. A draft with proposed criteria is approved
//! or sent back for changes from there.

use dusk_protocol::thread::{Goal, GoalStatus};

use super::{Action, App, Focus, Tone};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GoalButton {
    /// Approves the proposed criteria as they are shown, and starts the work.
    Approve,
    /// Puts the start of a message asking for other criteria in the box.
    Change,
    Pause,
    Resume,
    Clear,
}

/// What can be done with `goal` now, by its buttons.
pub fn buttons(goal: &Goal) -> &'static [GoalButton] {
    use GoalButton::*;
    match goal.status {
        _ if !goal.approved && goal.criteria.is_empty() => &[Clear],
        _ if !goal.approved => &[Approve, Change, Clear],
        GoalStatus::Active => &[Pause, Clear],
        GoalStatus::Paused => &[Resume, Clear],
        GoalStatus::Done => &[Clear],
    }
}

impl GoalButton {
    pub fn label(self) -> &'static str {
        match self {
            Self::Approve => " approve ",
            Self::Change => " change ",
            Self::Pause => " pause ",
            Self::Resume => " resume ",
            Self::Clear => " \u{00D7} ",
        }
    }
}

const CHANGE: &str = "Change the goal's criteria: ";

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
        let command = match button {
            // The revision on screen, so criteria the agent changed since are not approved.
            GoalButton::Approve => format!("/goal approve {}", goal.revision),
            GoalButton::Change => {
                self.input.set(CHANGE.to_owned());
                self.focus = Focus::Input;
                return Action::None;
            }
            GoalButton::Pause => "/goal pause".into(),
            GoalButton::Resume => "/goal resume".into(),
            GoalButton::Clear => "/goal clear".into(),
        };
        Action::Send(command)
    }
}
