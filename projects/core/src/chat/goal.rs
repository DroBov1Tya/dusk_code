//! A thread's standing goal. `/goal` and the `goal` tool both change it through
//! `apply`, so the user and the model have the same powers: set, pause, resume, clear,
//! and mark done. While a goal is active, every request carries it, and a turn that
//! ends without the goal done starts the next one, up to `max_goal_rounds`.

use anyhow::{Result, bail};

use crate::state::{Goal, GoalStatus, State};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalAction {
    Show,
    Set(String),
    Pause(Option<String>),
    Resume,
    Clear,
    Done(Option<String>),
}

impl GoalAction {
    /// What follows `/goal`: nothing shows it, a single action word acts, anything
    /// else is a new goal.
    pub fn from_command(argument: Option<&str>) -> Self {
        match argument.map(str::trim) {
            None | Some("") => Self::Show,
            Some("pause") => Self::Pause(None),
            Some("resume") => Self::Resume,
            Some("clear") => Self::Clear,
            Some("done") => Self::Done(None),
            Some(text) => Self::Set(text.to_owned()),
        }
    }

    /// Whether the thread should start working after it.
    pub fn starts_work(&self) -> bool {
        matches!(self, Self::Set(_) | Self::Resume)
    }
}

/// Changes the goal and says what changed, in a line for the user or the model.
pub async fn apply(
    state: &State,
    thread_id: &str,
    action: GoalAction,
) -> Result<(Option<Goal>, String)> {
    let current = state.goal(thread_id).await?;
    let (goal, note) = match action {
        GoalAction::Show => {
            let note = match &current {
                Some(goal) => describe(goal),
                None => "no goal; /goal <text> sets one".to_owned(),
            };
            (current, note)
        }
        GoalAction::Set(text) => {
            let goal = state.set_goal(thread_id, &text).await?;
            (Some(goal), "goal set".to_owned())
        }
        GoalAction::Clear => {
            let note = if state.clear_goal(thread_id).await? {
                "goal cleared"
            } else {
                "no goal to clear"
            };
            (None, note.to_owned())
        }
        GoalAction::Pause(reason) => change(state, current, GoalStatus::Paused, reason).await?,
        GoalAction::Resume => change(state, current, GoalStatus::Active, None).await?,
        GoalAction::Done(result) => change(state, current, GoalStatus::Done, result).await?,
    };
    Ok((goal, note))
}

async fn change(
    state: &State,
    current: Option<Goal>,
    status: GoalStatus,
    note: Option<String>,
) -> Result<(Option<Goal>, String)> {
    let Some(goal) = current else {
        bail!("no goal; /goal <text> sets one");
    };
    if goal.status == status {
        let note = format!("goal is {} already", status.name());
        return Ok((Some(goal), note));
    }
    let goal = state
        .set_goal_status(&goal.thread_id, status, note.as_deref())
        .await?;
    let said = match status {
        GoalStatus::Active => "goal resumed",
        GoalStatus::Paused => "goal paused",
        GoalStatus::Done => "goal done",
    };
    Ok((goal, said.to_owned()))
}

/// The goal in one line: status, rounds and text.
pub fn describe(goal: &Goal) -> String {
    let mut line = format!(
        "goal {} (round {}): {}",
        goal.status.name(),
        goal.rounds,
        goal.text
    );
    if let Some(note) = &goal.note {
        line.push_str(&format!(" | {note}"));
    }
    line
}

/// What every request of a thread with an active goal ends with.
pub fn reminder(goal: &Goal, max_rounds: u32) -> String {
    format!(
        "Standing goal (round {} of {max_rounds}): {}\n\
         Work toward it until it is fully done; the next round starts by itself when \
         this turn ends. When it is done, call the goal tool with action done and a short \
         result. Pause it with a reason if you need the user.",
        goal.rounds, goal.text
    )
}

/// The message that starts a turn for the goal.
pub fn kickoff(goal: &Goal, max_rounds: u32) -> String {
    if goal.rounds == 0 {
        format!("Goal: {}", goal.text)
    } else {
        format!(
            "Continue toward the goal (round {} of {max_rounds}): {}",
            goal.rounds, goal.text
        )
    }
}
