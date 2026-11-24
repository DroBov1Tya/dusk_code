//! What a member's turns do to its task. The member's runner reports each event here
//! instead of writing to the lead: a turn that starts makes the task running, one that
//! completes hands it in (or blocks it, when the member says so), one that fails or is
//! cut off releases it. Every move is the store's, checked there against the owner.
//!
//! A member's answer is untrusted text. It decides one thing, whether its own task is
//! handed in or blocked, and gives the words quoted in one line to the lead. Its footer
//! is kept as the task's report, which the lead reads with `show`. A reviewer's or the
//! critic's answer closes its row and is read by `review` or the end line.

use anyhow::Result;
use dusk_protocol::thread::{TaskDuty, TaskStatus};
use tracing::warn;

use super::{clean, dispatch, events, gate, lead_lines, lease, outside, review};
use crate::{
    message::plain::plain,
    runtime::{Launcher, ThreadEvent},
    state::{GoalCheck, MAX_RESULT, PlanError, PlanTask, Thread, Turn, TurnStatus},
};

/// The lines at the end of an answer that are read: `Status:`, `Result:`, `Checked:`,
/// `Not checked:` and `Open:`, as the card asks for them.
const FOOTER_LINES: usize = 5;
/// The most characters of a member's words kept as a task's note.
const MAX_NOTE: usize = 300;

/// What a member says of its task in its footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Status {
    Done,
    Blocked,
    /// It stopped half way; only a wind-down asks for it, and it counts as blocked.
    Partial,
    /// No `Status:` line among the last lines: nobody can tell what it claims.
    Missing,
}

/// What a member's final answer says, read from its last lines only: a `Status:` line
/// anywhere above them, such as in a file it quotes, says nothing.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Report {
    pub status: Status,
    /// What it needs, when it is blocked.
    pub blocked: Option<String>,
    /// Its `Result:` line, else the last line it wrote, quoted for a line to the lead.
    pub result: String,
    /// Its `Not checked:` line, quoted; `None` when it is missing or says nothing.
    pub not_checked: Option<String>,
    /// The footer as written, each line made plain: what the task keeps as its report.
    pub footer: String,
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
        let status = match field("Status:").map(|status| status.trim_end_matches('.')) {
            None => Status::Missing,
            Some(word) if word.eq_ignore_ascii_case("blocked") => Status::Blocked,
            Some(word) if word.eq_ignore_ascii_case("partial") => Status::Partial,
            Some(_) => Status::Done,
        };
        let blocked = matches!(status, Status::Blocked | Status::Partial).then(|| {
            let open = field("Open:").filter(|open| !open.is_empty());
            lead_lines::quoted(open.unwrap_or("it did not say what it needs"), MAX_NOTE)
        });
        let result = field("Result:")
            .or(footer.last().copied())
            .unwrap_or("(no answer)");
        let not_checked = field("Not checked:")
            .filter(|said| !is_nothing(said))
            .map(|said| lead_lines::quoted(said, MAX_NOTE / 2));
        let kept: Vec<String> = footer.iter().map(|line| plain(line)).collect();
        Self {
            status,
            blocked,
            result: lead_lines::quoted(result, MAX_NOTE),
            not_checked,
            footer: kept.join("\n").chars().take(MAX_RESULT).collect(),
        }
    }

    /// The result as the lead reads it: what works, and what was not checked.
    pub fn line(&self) -> String {
        match &self.not_checked {
            Some(not_checked) => format!("{}; not checked: {not_checked}", self.result),
            None => self.result.clone(),
        }
    }
}

/// Empty, `nothing`, `none`, `n/a`: a footer line that has nothing to say.
fn is_nothing(said: &str) -> bool {
    let said = said.trim_end_matches('.').trim();
    said.is_empty()
        || ["nothing", "none", "n/a", "-"]
            .iter()
            .any(|word| said.eq_ignore_ascii_case(word))
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
    // From its first event on this runner moves the member's task; recovery leaves
    // the member to it.
    {
        let mut served = launcher.team.served.lock();
        if !served.contains(&member.id) {
            served.insert(member.id.clone());
        }
    }
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
pub(super) async fn completed(
    launcher: &Launcher,
    lead: &str,
    member: &Thread,
    turn: &Turn,
) -> Result<bool> {
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
    let task = state.plan_task(lead, &held.task_id).await?;
    if task.duty != TaskDuty::Work {
        return duty_done(launcher, lead, member, &task, &answer).await;
    }
    let report = Report::of(&answer);
    // A footer can carry what a build printed: cleaned before it is kept.
    let footer = clean::text(launcher, lead, &report.footer).await;
    let footer: String = footer.chars().take(MAX_RESULT).collect();
    state
        .report_task(&member.id, &held.task_id, &footer)
        .await?;
    let moved = match &report.blocked {
        Some(note) => {
            let note = clean::note(launcher, lead, note).await;
            state
                .block_task(&member.id, &held.task_id, None, &note)
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
    if report.blocked.is_some() {
        lead_lines::blocked(launcher, lead, member, &task).await;
        events::task(launcher, lead, &task).await;
        return Ok(true);
    }
    let result = report.line();
    // A change outside the task's files is the lead's to judge, whatever its gate; so is
    // an answer whose footer does not say how it ended.
    let outside = outside::line(launcher, &member.id, &task).await?;
    let held_why = match (&outside, report.status) {
        (Some(outside), _) => Some(match &task.gate {
            None => format!("no gate; {outside}"),
            Some(_) => format!("its gate was not run: it {outside}"),
        }),
        (None, Status::Missing) => Some("its answer has no footer with a status".to_owned()),
        (None, _) => None,
    };
    let task = match (held_why, &task.gate) {
        (Some(why), _) => {
            let why = clean::note(launcher, lead, &why).await;
            let held = state.hold_for_lead(lead, &task.task_id, &why).await?;
            lead_lines::review(launcher, lead, &member.id, &held, &result, &why).await;
            held
        }
        (None, None) if review::due(launcher, &member.id, &task).await? => {
            review::begin(launcher, lead, &task).await?
        }
        (None, None) => {
            lead_lines::review(launcher, lead, &member.id, &task, &result, "no gate").await;
            task
        }
        (None, Some(GoalCheck::Command { .. })) => {
            gate::begin(launcher, lead, &member.id, &task, &result).await?
        }
        // It stays in review until the user's ConfirmTask passes it. Clients show it,
        // and the lead is told once so the chat says what the user is asked for.
        (None, Some(GoalCheck::UserConfirmation)) => {
            lead_lines::confirm(launcher, lead, member, &task, &result).await;
            task
        }
    };
    events::task(launcher, lead, &task).await;
    Ok(true)
}

/// A reviewer's or the critic's turn completed: its row is closed with what it answered,
/// whatever its footer says, and a review settles the task it reviewed.
async fn duty_done(
    launcher: &Launcher,
    lead: &str,
    member: &Thread,
    row: &PlanTask,
    answer: &str,
) -> Result<bool> {
    let kept = clean::text(launcher, lead, &review::kept(answer)).await;
    let kept: String = kept.chars().take(MAX_RESULT).collect();
    let closed = launcher
        .state
        .close_duty(&member.id, &row.task_id, &kept)
        .await;
    let row = match closed {
        Ok(row) => row,
        Err(PlanError::NotOwner { .. } | PlanError::Illegal { .. }) => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    events::task(launcher, lead, &row).await;
    if row.duty == TaskDuty::Review {
        review::settle(launcher, lead, &member.id, &row).await?;
    }
    Ok(true)
}
