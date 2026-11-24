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

use super::{
    clean,
    dispatch::{self, Loss},
    events, footer, gate, lead_lines, lease, outside, review,
};
use crate::{
    message::plain::plain,
    runtime::{Launcher, ThreadEvent},
    state::{GoalCheck, MAX_RESULT, PlanError, PlanTask, Thread, Turn, TurnStatus},
};

/// The labels of the lines that end a member's answer, as the card asks for them.
const LABELS: [&str; 5] = ["Status:", "Result:", "Checked:", "Not checked:", "Open:"];
/// The most characters of a member's words kept as a task's note.
const MAX_NOTE: usize = 300;

/// What a member says of its task in its footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Status {
    Done,
    Blocked,
    /// It stopped half way; only a wind-down asks for it, and it counts as blocked.
    Partial,
    /// No `Status:` line in the footer, or one that is not one of the words asked for:
    /// nobody can tell what it claims.
    Missing,
}

/// What a member's final answer says, read from its footer only (`footer`): a `Status:`
/// line above it, such as in a file it quotes, says nothing.
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
        let footer = footer::read(answer, &LABELS, &[]);
        let status = match footer.word("Status:", &["done", "blocked", "partial"]) {
            Some(word) if word == "done" => Status::Done,
            Some(word) if word == "blocked" => Status::Blocked,
            Some(_) => Status::Partial,
            None => Status::Missing,
        };
        let blocked = matches!(status, Status::Blocked | Status::Partial).then(|| {
            let open = footer.text("Open:").filter(|open| !open.is_empty());
            let open = open.as_deref().unwrap_or("it did not say what it needs");
            lead_lines::quoted(open, MAX_NOTE)
        });
        let last = answer.lines().map(str::trim).rfind(|line| !line.is_empty());
        let result = footer
            .text("Result:")
            .filter(|result| !result.is_empty())
            .or(last.map(str::to_owned))
            .unwrap_or_else(|| "(no answer)".to_owned());
        let not_checked = footer
            .text("Not checked:")
            .filter(|said| !is_nothing(said))
            .map(|said| lead_lines::quoted(&said, MAX_NOTE / 2));
        // Without a footer, the last lines are what the lead reads under the task.
        let lines = match footer.lines.is_empty() {
            false => footer.lines,
            true => {
                let all: Vec<&str> = answer
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .collect();
                all[all.len().saturating_sub(LABELS.len())..].to_vec()
            }
        };
        let kept: Vec<String> = lines.iter().map(|line| plain(line)).collect();
        Self {
            status,
            blocked,
            result: lead_lines::quoted(&result, MAX_NOTE),
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
            dispatch::lost(launcher, lead, &member.id, &note, Loss::Infra(None)).await
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
            // Why it failed is kept with the turn: the provider's fault is not the
            // member's, and does not count against its task.
            let failure = launcher.state.last_failure(&member.id).await?;
            let kind = failure.and_then(|failure| failure.provider.map(|failed| failed.kind));
            dispatch::lost(launcher, lead, &member.id, &note, Loss::of_failure(kind)).await
        }
        // The core is shutting down. The lease runs out and the next start goes on.
        TurnStatus::Interrupted if launcher.mailboxes.is_closed() => Ok(false),
        // Stopped from outside while it held its task: only the user does that, by
        // stopping the chat or this member. The core does not start again what the
        // user stopped, and the turns it would wind down are cut already, so the team
        // ends as stopped at once.
        TurnStatus::Interrupted if announced => {
            // A task still claimed was given back to this member after the turn began:
            // the turn is the one the core cut off when it took the task away (a lease
            // that ran out), not the user's stop of the new round.
            let held = lease::at_work(&launcher.state, lead, &member.id).await?;
            if held.is_some_and(|held| held.status != TaskStatus::Claimed) {
                launcher.team_cancel(lead).await?;
            }
            Ok(false)
        }
        // A restart found the turn cut off where it could not go on: not the member's
        // loss.
        TurnStatus::Interrupted => {
            let note = "a restart cut its turn off";
            dispatch::lost(launcher, lead, &member.id, note, Loss::Cut).await
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
    // The provider answered: losses to it in a row start again.
    dispatch::infra_passed(launcher, lead, &held.task_id);
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
    // A team that winds down asked for this answer: a task its member did not finish
    // goes back to open with the footer as its hand-over, which the next member's card
    // shows. The attempt is not counted; the core stopped it.
    let winding = state
        .team_run(lead)
        .await?
        .is_some_and(|run| run.winding.is_some());
    if winding && report.status != Status::Done {
        let note = "handed over when the team stopped; its report says where it stands";
        let task = match state.return_task(&member.id, &held.task_id, note).await {
            Ok(task) => task,
            Err(PlanError::NotOwner { .. } | PlanError::Illegal { .. }) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        events::task(launcher, lead, &task).await;
        return Ok(true);
    }
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
        (None, Status::Missing) => {
            Some("its answer gives no clear status in its footer".to_owned())
        }
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
        // A team that winds down runs no gate: the next run starts it (`gate::resume`).
        (None, Some(GoalCheck::Command { .. })) if winding => task,
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
