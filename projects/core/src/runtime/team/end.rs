//! How a team ends: every task is done, the lead or the user stopped it, the lead went
//! quiet at the cap of agent messages, or it stalled. Whatever the reason, the members
//! at work are cancelled, their tasks go back to open, and the lead is told once.

use dusk_protocol::thread::{TaskStatus, TeamEnd};
use tracing::warn;

use super::{allow, events, gate, lead_lines};
use crate::{
    runtime::{Launcher, agent::Input},
    state::{Delivery, PlanTask, TeamError, TeamRun},
};

/// Ends the chat's running team with `why`; answers the tasks that went back to open.
/// The caller holds the pass lock.
///
/// The order: the members are read first, since a returned task has no owner; then the
/// end is stored, which returns their tasks; only then are their runners cancelled, so
/// a turn cut off by this end finds its task gone and is not counted as a lost attempt;
/// then the lead is told. A stop between any two leaves an ended team: nothing is given
/// out, a member takes no further step (`lease::may_step`), and the next `run` returns
/// what was left at work.
pub(super) async fn end(
    launcher: &Launcher,
    lead: &str,
    why: TeamEnd,
    now: i64,
) -> Result<Vec<PlanTask>, TeamError> {
    let state = &launcher.state;
    let members = state.team_members(lead).await?;
    let ended = state.end_team(lead, why, now).await;
    if matches!(ended, Ok(_) | Err(TeamError::Plan(_))) {
        for member in &members {
            if matches!(member.status, TaskStatus::Claimed | TaskStatus::Running) {
                cancel_member(launcher, &member.thread_id).await;
            }
        }
    }
    let (run, returned) = ended?;
    allow::forget(launcher, lead);
    gate::stop(launcher, lead);
    for task in &returned {
        events::task(launcher, lead, task).await;
    }
    events::team(launcher, lead, now, true).await;
    tell(launcher, &run, why, now).await;
    Ok(returned)
}

/// Stops a member's runner. One that waits for a free slot has no runner: its card is
/// dropped instead, so it never starts on a task it no longer has.
pub(super) async fn cancel_member(launcher: &Launcher, thread_id: &str) {
    match launcher.mailboxes.get(thread_id).await {
        Some(mailbox) => {
            mailbox.send(Input::Cancel);
        }
        None => {
            if let Err(error) = launcher.state.drop_pending(thread_id).await {
                warn!("cannot drop what waits for member {thread_id}: {error:#}");
            }
        }
    }
}

/// `Team ended: done. 7 of 7 tasks; 212 steps, 1.31M tokens, 18 min.` Stored under the
/// run's own uid, so an end is told once. A stopped team does not wake its lead: the
/// user stopped the work, and the line is read with the next request.
async fn tell(launcher: &Launcher, run: &TeamRun, why: TeamEnd, now: i64) {
    let lead = &run.thread_id;
    let state = &launcher.state;
    let (Ok(counts), Ok(Some(team))) = (state.team_counts(lead).await, state.team(lead, now).await)
    else {
        return warn!("cannot read the ended team of thread {lead}");
    };
    let reason = match why {
        TeamEnd::Done => "done",
        // Only older cores ended a team on a budget.
        TeamEnd::Steps | TeamEnd::Tokens | TeamEnd::Time => "its budget was spent",
        TeamEnd::Stalled => "stalled, nothing could start",
        TeamEnd::Stopped => "stopped",
        TeamEnd::Quiet => "quiet, you were written to 30 times without the user",
    };
    let left = match why {
        TeamEnd::Done => "",
        _ => " Tasks at work went back to open; run goes on from here.",
    };
    let line = format!(
        "Team ended: {reason}. {} of {} tasks; {} steps, {} tokens, {} min.{left}",
        counts.done,
        counts.total,
        team.used.steps,
        tokens(team.used.tokens),
        team.used.minutes
    );
    let uid = format!("team:{lead}:{}:end", run.started_at);
    let wake = why != TeamEnd::Stopped;
    lead_lines::tell(launcher, lead, lead, &uid, &line, wake).await;
}

/// `1.31M`, `212k`, `950`.
fn tokens(count: u64) -> String {
    match count {
        1_000_000.. => format!("{}.{:02}M", count / 1_000_000, count % 1_000_000 / 10_000),
        1_000.. => format!("{}k", count / 1_000),
        _ => count.to_string(),
    }
}

/// After a pass gave out what it could: the team ends when every task is done, and the
/// stall rule applies when nothing is at work and nothing is ready. The caller holds
/// the pass lock.
///
/// The stall rule, from the tables: the lead gets one line; once a turn of the lead that
/// read it has ended and no task changed since, the team ends as stalled. A line the
/// lead was just woken with for this very state of the plan (a task waits for it,
/// failed, is blocked) is that one line; a second would say the same.
pub(super) async fn settle(launcher: &Launcher, run: &TeamRun, now: i64) -> Result<(), TeamError> {
    let lead = &run.thread_id;
    let state = &launcher.state;
    let counts = state.team_counts(lead).await?;
    if counts.total > 0 && counts.done == counts.total {
        end(launcher, lead, TeamEnd::Done, now).await?;
        return Ok(());
    }
    if counts.at_work > 0 || counts.ready > 0 {
        return Ok(());
    }
    // A task whose gate runs, or that waits for the user's confirmation, is no stall:
    // the gate ends by itself and the user was asked. The lead can do nothing about
    // either, so it is not woken to end the team; a gate has its own time limit
    // (`limits.team_gate_seconds`), and the user's Stop ends the wait for them.
    if counts.review > counts.for_lead {
        return Ok(());
    }
    let mark = lead_lines::plan_mark(state, lead).await?;
    let stall = format!("team:{lead}:{}:stall:{mark}", run.started_at);
    let watched = match lead_lines::last_told(launcher, lead) {
        Some(told) if told.mark == mark => told.uid,
        _ => stall.clone(),
    };
    match state.delivery(&watched).await? {
        Delivery::Read { answered: true } => {
            end(launcher, lead, TeamEnd::Stalled, now).await?;
        }
        Delivery::Read { answered: false } | Delivery::Waiting => {}
        Delivery::None => {
            let line = lead_lines::stall_line(&counts);
            lead_lines::tell(launcher, lead, lead, &stall, &line, true).await;
        }
    }
    Ok(())
}
