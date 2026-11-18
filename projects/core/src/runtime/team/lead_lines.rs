//! What the lead is told: one line when a task waits for it, failed or is blocked, and
//! one when the team ends or cannot go on. Tasks the core or the user passed meanwhile
//! are named at the start of the next of those. A line is 400 bytes at most; whatever a
//! member wrote is made plain and cut before it goes into one, so a member's answer can
//! neither forge a line nor fill the lead's context.
//!
//! Each line is stored under a uid of its task and state, so a pass that runs again, as
//! after a restart, tells the lead nothing twice.

use std::path::Path;

use tracing::warn;

use super::clean;
use crate::{
    message::plain::plain,
    runtime::Launcher,
    state::{PlanStatus, PlanTask, State, TeamCounts, Thread, timestamp_ms},
    tools::short_task_id,
};

/// The most bytes of one line.
pub(super) const MAX_LINE: usize = 400;
/// The most characters of a member's text in a line.
const MAX_QUOTED: usize = 300;
/// Tasks named as done meanwhile at the start of a line.
const MAX_FOLDED: usize = 6;

/// The line a lead was last woken with: what the stall rule waits on.
#[derive(Clone)]
pub(super) struct Told {
    pub uid: String,
    /// The plan's newest change when the line was written.
    pub mark: i64,
    /// When it was written; tasks done after it are named in the next line.
    at: i64,
}

/// `text` as one plain piece of a line: a member's or a program's words, made plain
/// and cut to `max` characters. Only the start is read, so megabytes cost nothing.
pub(super) fn quoted(text: &str, max: usize) -> String {
    let start: String = text.chars().take(max.saturating_mul(4)).collect();
    let whole = plain(&start);
    let mut cut: String = whole.chars().take(max).collect();
    if cut.len() < whole.len() {
        cut.push('…');
    }
    cut.trim_end_matches('.').to_owned()
}

/// `head`, `text` and `tail` as one line of `MAX_LINE` bytes at most: `text`, the only
/// part of unknown length, is what gets cut.
fn line(head: &str, text: &str, tail: &str) -> String {
    let room = MAX_LINE.saturating_sub(head.len() + tail.len());
    let mut end = text.len().min(room);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{head}{}{tail}", &text[..end])
}

/// `text` at `MAX_LINE` bytes at most, never inside a character.
fn cut(text: &str) -> &str {
    let mut end = text.len().min(MAX_LINE);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

pub(super) fn review_line(folded: &str, task: &PlanTask, result: &str, why: &str) -> String {
    let head = format!(
        "{folded}#{} waits for you: {why}. Result: ",
        short_task_id(&task.task_id)
    );
    let tail = ". Accept or reject with tasks review.";
    line(&head, &quoted(result, MAX_QUOTED), tail)
}

pub(super) fn confirm_line(folded: &str, task: &PlanTask, result: &str) -> String {
    let head = format!(
        "{folded}#{} waits for the user to confirm it. Result: ",
        short_task_id(&task.task_id)
    );
    let tail = ". Tell the user what to look at; only they can confirm it, the team goes on.";
    line(&head, &quoted(result, MAX_QUOTED), tail)
}

pub(super) fn failed_line(folded: &str, task: &PlanTask) -> String {
    let head = format!("{folded}#{} failed: ", short_task_id(&task.task_id));
    // A task that could not be given out at all failed before any attempt.
    let tail = match task.attempts {
        0 => ". Reopen it, change the plan, or stop.".to_owned(),
        attempts => format!(" after {attempts} attempts. Reopen it, change the plan, or stop."),
    };
    line(&head, &quoted(&task.note, MAX_QUOTED), &tail)
}

pub(super) fn blocked_line(folded: &str, task: &PlanTask) -> String {
    let head = format!("{folded}#{} blocked: ", short_task_id(&task.task_id));
    let tail = ". Answer by reopening it and editing its brief, or fail it.";
    line(&head, &quoted(&task.note, MAX_QUOTED), tail)
}

/// `Nothing can start: 1 waits for you, 1 failed, 2 behind them.`
pub(super) fn stall_line(counts: &TeamCounts) -> String {
    let waits = |count: u32, whom: &str| match count {
        1 => format!("1 waits for {whom}"),
        count => format!("{count} wait for {whom}"),
    };
    let behind = counts.total.saturating_sub(
        counts.done + counts.failed + counts.review + counts.at_work + counts.ready,
    );
    let mut parts = Vec::new();
    if counts.for_lead > 0 {
        parts.push(waits(counts.for_lead, "you"));
    }
    let others = counts.review.saturating_sub(counts.for_lead);
    if others > 0 {
        parts.push(waits(others, "the user or a gate"));
    }
    if counts.failed > 0 {
        parts.push(format!("{} failed", counts.failed));
    }
    if behind > 0 {
        parts.push(format!("{behind} behind them"));
    }
    if parts.is_empty() {
        return "Nothing can start: the plan has no task to give out.".into();
    }
    format!("Nothing can start: {}.", parts.join(", "))
}

/// A task waits for the lead's review, for the reason `why`.
pub(super) async fn review(
    launcher: &Launcher,
    lead: &str,
    member: &str,
    task: &PlanTask,
    result: &str,
    why: &str,
) {
    let folded = folded(launcher, lead).await;
    let line = review_line(&folded, task, result, why);
    tell(launcher, lead, member, &uid(task), &line, true).await;
}

/// A task waits for the user's confirmation.
pub(super) async fn confirm(
    launcher: &Launcher,
    lead: &str,
    member: &Thread,
    task: &PlanTask,
    result: &str,
) {
    let folded = folded(launcher, lead).await;
    let line = confirm_line(&folded, task, result);
    tell(launcher, lead, &member.id, &uid(task), &line, true).await;
}

/// A task failed: its member's turns were lost three times, or its gate failed after
/// its last rework.
pub(super) async fn failed(launcher: &Launcher, lead: &str, task: &PlanTask) {
    let folded = folded(launcher, lead).await;
    let from = task.owner_thread_id.as_deref().unwrap_or(lead);
    let line = failed_line(&folded, task);
    tell(launcher, lead, from, &uid(task), &line, true).await;
}

/// A member said it cannot go on.
pub(super) async fn blocked(launcher: &Launcher, lead: &str, member: &Thread, task: &PlanTask) {
    let folded = folded(launcher, lead).await;
    let line = blocked_line(&folded, task);
    tell(launcher, lead, &member.id, &uid(task), &line, true).await;
}

/// A line's uid: the task, where it stands, and which round of it this is.
fn uid(task: &PlanTask) -> String {
    format!(
        "line:{}:{}:{}.{}",
        task.task_id,
        task.status.as_str(),
        task.attempts,
        task.reworks
    )
}

/// Stores `line` for the lead under `uid`, once. `wake` starts the lead's next turn on
/// it; without it the line waits for the lead's next request, as after a stop, which
/// must not start work again.
pub(super) async fn tell(
    launcher: &Launcher,
    lead: &str,
    from: &str,
    uid: &str,
    line: &str,
    wake: bool,
) {
    let state = &launcher.state;
    // The one way a line reaches the lead: a member's or a program's words in it are
    // cleared of secrets here, whoever built the line.
    let cleaned = clean::text(launcher, lead, line).await;
    let line = cut(&cleaned);
    let stored = match wake {
        // A chat is woken, never started, so no folder is used.
        true => {
            launcher
                .deliver_once(lead, from, uid, line, Path::new(""))
                .await
        }
        false => state.add_pending_once(lead, uid, line, Some(from)).await,
    };
    if let Err(error) = stored {
        return warn!("cannot tell thread {lead} of its team: {error:#}");
    }
    if !wake {
        return;
    }
    let (Ok(mark), Ok(at)) = (plan_mark(state, lead).await, timestamp_ms()) else {
        return;
    };
    let told = Told {
        uid: uid.to_owned(),
        mark,
        at,
    };
    launcher.team.told.lock().insert(lead.to_owned(), told);
}

/// When the plan last changed: the newest `updated_at` of its tasks.
pub(super) async fn plan_mark(state: &State, lead: &str) -> anyhow::Result<i64> {
    let plan = state.plan(lead).await?;
    Ok(plan.iter().map(|task| task.updated_at).max().unwrap_or(0))
}

/// The line the lead was last woken with, if this process wrote one.
pub(super) fn last_told(launcher: &Launcher, lead: &str) -> Option<Told> {
    launcher.team.told.lock().get(lead).cloned()
}

/// `Done meanwhile: #a #b. `: the tasks a gate or the user passed since the lead was
/// last told anything, which it was not woken for. Empty when there is none. After a
/// restart the last line is not known, and the tasks done since the run began are named.
async fn folded(launcher: &Launcher, lead: &str) -> String {
    let state = &launcher.state;
    let since = match last_told(launcher, lead) {
        Some(told) => told.at,
        None => match state.team_run(lead).await {
            Ok(Some(run)) => run.started_at,
            _ => return String::new(),
        },
    };
    let Ok(plan) = state.plan(lead).await else {
        return String::new();
    };
    // The lead passes a task without a gate itself, and knows.
    let done: Vec<&PlanTask> = plan
        .iter()
        .filter(|task| task.status == PlanStatus::Done && task.gate.is_some())
        .filter(|task| task.updated_at > since)
        .collect();
    if done.is_empty() {
        return String::new();
    }
    let mut ids: Vec<String> = done
        .iter()
        .take(MAX_FOLDED)
        .map(|task| format!("#{}", short_task_id(&task.task_id)))
        .collect();
    if done.len() > MAX_FOLDED {
        ids.push(format!("and {} more", done.len() - MAX_FOLDED));
    }
    format!("Done meanwhile: {}. ", ids.join(" "))
}
