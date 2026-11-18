//! What a member's turns do to its task. The member's runner reports each event here
//! instead of writing to the lead: a turn that starts makes the task running, one that
//! completes hands it in (or blocks it, when the member says so), one that fails or is
//! cut off releases it. Every move is the store's, checked there against the owner.
//!
//! A member's answer is untrusted text. It decides one thing, whether its own task is
//! handed in or blocked, and gives the words quoted in one line to the lead.

use anyhow::Result;
use dusk_protocol::thread::TaskStatus;
use tracing::warn;

use super::{dispatch, events, gate, lead_lines, lease};
use crate::{
    runtime::{Launcher, ThreadEvent},
    state::{GoalCheck, PlanError, Thread, Turn, TurnStatus},
};

/// The lines at the end of an answer that are read: `Status:`, `Result:`, `Checked:`
/// and `Open:`, as the member's prompt asks for them.
const FOOTER_LINES: usize = 4;
/// The most characters of a member's words kept as a task's note.
const MAX_NOTE: usize = 300;

/// What a member's final answer says, read from its last lines only: a `Status:` line
/// anywhere above them, such as in a file it quotes, says nothing.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Report {
    /// What it needs, when its status is `blocked`.
    pub blocked: Option<String>,
    /// Its `Result:` line, else the last line it wrote.
    pub result: String,
}

impl Report {
    pub fn of(answer: &str) -> Self {
        let mut footer: Vec<&str> = answer
            .lines()
            .rev()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(FOOTER_LINES)
            .collect();
        footer.reverse();
        let field = |name: &str| {
            footer
                .iter()
                .rev()
                .find_map(|line| line.strip_prefix(name))
                .map(str::trim)
        };
        let is_blocked = field("Status:")
            .is_some_and(|status| status.trim_end_matches('.').eq_ignore_ascii_case("blocked"));
        let blocked = is_blocked.then(|| {
            let open = field("Open:").filter(|open| !open.is_empty());
            lead_lines::quoted(open.unwrap_or("it did not say what it needs"), MAX_NOTE)
        });
        let result = field("Result:")
            .or(footer.last().copied())
            .unwrap_or("(no answer)");
        Self {
            blocked,
            result: lead_lines::quoted(result, MAX_NOTE),
        }
    }
}

/// An event of a member's runner. `announced` says a turn was seen at work in this
/// runner's life, which tells a stop by the user from a turn a restart cut off.
pub(crate) async fn member_event(
    launcher: &Launcher,
    member: &Thread,
    event: &ThreadEvent,
    announced: bool,
) {
    let Some(lead) = member.parent_thread_id.as_deref() else {
        return;
    };
    let moved = match event {
        ThreadEvent::TurnStarted(turn) | ThreadEvent::TurnRunning(turn) => {
            started(launcher, lead, member, turn).await
        }
        ThreadEvent::TurnFinished(turn) => finished(launcher, lead, member, turn, announced).await,
        // The runner itself stopped, such as on a database error.
        ThreadEvent::Failed(error) => {
            let note = format!(
                "its runner stopped: {}",
                lead_lines::quoted(error, MAX_NOTE)
            );
            dispatch::lost(launcher, lead, &member.id, &note).await
        }
        _ => return,
    };
    match moved {
        // A task changed: what became ready is given out, the end is looked for.
        Ok(true) => launcher.team_changed(lead).await,
        Ok(false) => {}
        Err(error) => warn!("member {} of thread {lead}: {error:#}", member.id),
    }
}

/// claimed to running. A turn that goes on after a rework finds its task running.
async fn started(launcher: &Launcher, lead: &str, member: &Thread, turn: &Turn) -> Result<bool> {
    let state = &launcher.state;
    let Some(held) = lease::at_work(state, lead, &member.id).await? else {
        return Ok(false);
    };
    match held.status {
        TaskStatus::Claimed => {
            let task = state
                .start_task(&member.id, &held.task_id, &turn.turn_id)
                .await?;
            events::task(launcher, lead, &task).await;
        }
        // The turn that redoes a task sent back is the task's as much as the first.
        _ => {
            state
                .mark_task_turn(&member.id, &held.task_id, &turn.turn_id)
                .await?
        }
    }
    Ok(false)
}

async fn finished(
    launcher: &Launcher,
    lead: &str,
    member: &Thread,
    turn: &Turn,
    announced: bool,
) -> Result<bool> {
    match turn.status {
        TurnStatus::Running => Ok(false),
        TurnStatus::Completed => completed(launcher, lead, member, turn).await,
        TurnStatus::Failed => {
            let error = turn.error_message.as_deref().unwrap_or("no reason given");
            let note = format!("its turn failed: {}", lead_lines::quoted(error, MAX_NOTE));
            dispatch::lost(launcher, lead, &member.id, &note).await
        }
        // The core is shutting down. The lease runs out and the next start goes on.
        TurnStatus::Interrupted if launcher.mailboxes.is_closed() => Ok(false),
        // Stopped from outside while it held its task: only the user does that, by
        // stopping the chat or this member. The core does not start again what the
        // user stopped, so the team ends as stopped.
        TurnStatus::Interrupted if announced => {
            if lease::at_work(&launcher.state, lead, &member.id)
                .await?
                .is_some()
            {
                launcher.team_stop(lead).await?;
            }
            Ok(false)
        }
        // A restart found the turn cut off where it could not go on.
        TurnStatus::Interrupted => {
            dispatch::lost(launcher, lead, &member.id, "a restart cut its turn off").await
        }
    }
}

/// The member's turn completed: its answer is the hand-in, or says it is blocked.
async fn completed(launcher: &Launcher, lead: &str, member: &Thread, turn: &Turn) -> Result<bool> {
    let state = &launcher.state;
    let Some(held) = lease::at_work(state, lead, &member.id).await? else {
        return Ok(false);
    };
    // The start was not seen, as when the turn began before a restart.
    match held.status {
        TaskStatus::Claimed => {
            state
                .start_task(&member.id, &held.task_id, &turn.turn_id)
                .await?;
        }
        _ => {
            state
                .mark_task_turn(&member.id, &held.task_id, &turn.turn_id)
                .await?
        }
    }
    let answer = launcher
        .answer(&member.id, turn.response_message_id.as_deref())
        .await;
    let report = Report::of(&answer);
    let moved = match &report.blocked {
        Some(note) => {
            state
                .block_task(&member.id, &held.task_id, None, note)
                .await
        }
        None => state.hand_in(&member.id, &held.task_id).await,
    };
    let task = match moved {
        Ok(task) => task,
        // The task left this member meanwhile: its lease ran out, or the team ended.
        Err(PlanError::NotOwner { .. } | PlanError::Illegal { .. }) => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let task = match (&report.blocked, &task.gate) {
        (Some(_), _) => {
            lead_lines::blocked(launcher, lead, member, &task).await;
            task
        }
        (None, None) => {
            lead_lines::review(launcher, lead, &member.id, &task, &report.result, "no gate").await;
            task
        }
        (None, Some(GoalCheck::Command { .. })) => {
            gate::begin(launcher, lead, &member.id, &task, &report.result).await?
        }
        // It stays in review until the user's ConfirmTask passes it. Clients show it,
        // and the lead is told once so the chat says what the user is asked for.
        (None, Some(GoalCheck::UserConfirmation)) => {
            lead_lines::confirm(launcher, lead, member, &task, &report.result).await;
            task
        }
    };
    events::task(launcher, lead, &task).await;
    Ok(true)
}
