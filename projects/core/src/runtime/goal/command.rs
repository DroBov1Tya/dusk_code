//! `/goal`: what the user typed, what it does to the chat's goal, and the messages that
//! keep the agent working toward it.

use anyhow::{Result, ensure};
use dusk_protocol::{system::now_ms, thread::MAX_GOAL_CHARS};

use crate::state::{Goal, State};

/// What `/goal` takes, besides a condition, to clear the goal.
const CLEAR_WORDS: [&str; 6] = ["clear", "stop", "off", "reset", "none", "cancel"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalAction {
    Show,
    Set(String),
    Clear,
}

impl GoalAction {
    pub fn from_command(argument: Option<&str>) -> Result<Self> {
        let text = argument.unwrap_or_default().trim();
        if text.is_empty() {
            return Ok(Self::Show);
        }
        if CLEAR_WORDS.contains(&text.to_lowercase().as_str()) {
            return Ok(Self::Clear);
        }
        ensure!(
            text.chars().count() <= MAX_GOAL_CHARS,
            "the goal is longer than {MAX_GOAL_CHARS} characters"
        );
        Ok(Self::Set(text.to_owned()))
    }
}

/// What a `/goal` did: the goal now, the line to show, and the message that starts work
/// on a new goal, which the caller queues.
pub struct Applied {
    pub goal: Option<Goal>,
    pub note: String,
    pub started: Option<String>,
}

/// Called only from the user's command path.
pub(crate) async fn apply(state: &State, thread_id: &str, action: GoalAction) -> Result<Applied> {
    let current = state.goal(thread_id).await?;
    Ok(match action {
        GoalAction::Show => Applied {
            note: match &current {
                Some(goal) => goal.status_text(now_ms()),
                None => "No goal set. /goal <condition> sets one.".into(),
            },
            goal: current,
            started: None,
        },
        GoalAction::Clear => Applied {
            note: match (&current, state.clear_goal(thread_id).await?) {
                (Some(goal), true) => format!("Goal cleared: {}", goal.text),
                _ => "No goal set".into(),
            },
            goal: None,
            started: None,
        },
        GoalAction::Set(text) => {
            let goal = state.set_goal(thread_id, &text).await?;
            Applied {
                note: match current {
                    Some(_) => "Goal replaced".into(),
                    None => "Goal set".into(),
                },
                started: Some(kickoff(&goal)),
                goal: Some(goal),
            }
        }
    })
}

/// Said to the model last at every step of an active goal.
pub fn reminder(goal: &Goal) -> Result<String> {
    let prompt = crate::release::resources::text("internal/GOAL.md")?;
    Ok(format!("{}\n\nGoal: {}", prompt.trim(), goal.text))
}

/// The message that starts work on a new goal. It never starts with `/`, so it is not
/// taken for a command.
pub fn kickoff(goal: &Goal) -> String {
    format!("Goal: {}", goal.text)
}

/// The message that starts the next turn of a goal not met yet.
pub fn continuation(reason: &str) -> String {
    format!("The goal is not met yet: {reason}\nKeep working toward it.")
}

/// The message that tries a turn again after an error the provider may get over.
pub fn retry(error: &str) -> String {
    format!("The last turn failed: {error}\nContinue toward the goal.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_of_clearing_clears_and_anything_else_is_a_condition() {
        assert_eq!(GoalAction::from_command(None).unwrap(), GoalAction::Show);
        assert_eq!(
            GoalAction::from_command(Some("  ")).unwrap(),
            GoalAction::Show
        );
        for word in ["clear", "STOP", "off", "reset", "none", "cancel"] {
            assert_eq!(
                GoalAction::from_command(Some(word)).unwrap(),
                GoalAction::Clear
            );
        }
        assert_eq!(
            GoalAction::from_command(Some("stop the flaky test")).unwrap(),
            GoalAction::Set("stop the flaky test".into())
        );
        let long = "x".repeat(MAX_GOAL_CHARS + 1);
        assert!(GoalAction::from_command(Some(&long)).is_err());
    }
}
