//! How a team ends: every task is done, the lead or the user stopped it, the lead went
//! quiet at the cap of agent messages, its provider kept failing, or it stalled. A stop,
//! the quiet cap and the provider first wind it down (`wind`): nothing new is given out
//! and the core, not the lead, asks each member at work to stop and hand over; the team
//! ends once they have, or when `limits.team_wrap_seconds` ran out. At the end the
//! members still at work are cancelled, their tasks go back to open, and the lead is
//! told once.

use std::path::Path;

use dusk_protocol::{
    model::ProviderErrorKind,
    thread::{TaskDuty, TaskStatus, TeamEnd, TeamMember, TeamStatus},
};
use tracing::warn;

use super::{allow, critic, dispatch, events, gate, lead_lines};
use crate::{
    runtime::{Launcher, agent::Input},
    state::{
        Delivery, PlanStatus, PlanTask, TeamError, TeamRun, WindWhy, Winding, current_wave,
        short_task_id, waves,
    },
};

/// What the core sends each member at work when its team winds down. The core's words,
/// not the lead's: the lead may be the reason it stops.
const WRAP: &str = "\
[the team is stopping; from the core, not the lead]
Stop now. Start no new change; leave the files compiling if you can. Answer with the footer:
Status: done if the task is finished, else partial
Result / Checked / Not checked / Open: what the next member needs to go on.";

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
    end_for(launcher, lead, why, None, now).await
}

/// `end`, with the provider's reason when it is why the team stops.
async fn end_for(
    launcher: &Launcher,
    lead: &str,
    why: TeamEnd,
    kind: Option<ProviderErrorKind>,
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
    tell(launcher, &run, why, kind, now).await;
    Ok(returned)
}

/// Winds the chat's running team down with `why`: nothing new is given out from now on,
/// each member at work on a task of the lead's is asked to stop and hand over, and the
/// team ends once they have (`wound`). A member that has not begun, and a review or the
/// critic, has nothing to hand over: its task goes back to open at once, its attempt not
/// counted. With nobody left at work, or when it already winds down, the team ends now:
/// a second stop does not wait. The caller holds the pass lock. Answers the tasks that
/// went back to open.
pub(super) async fn wind(
    launcher: &Launcher,
    lead: &str,
    why: WindWhy,
    now: i64,
) -> Result<Vec<PlanTask>, TeamError> {
    wind_for(launcher, lead, why, None, now).await
}

/// `wind`, with the provider's reason when the provider is why: the lead's line names
/// it.
pub(super) async fn wind_for(
    launcher: &Launcher,
    lead: &str,
    why: WindWhy,
    kind: Option<ProviderErrorKind>,
    now: i64,
) -> Result<Vec<PlanTask>, TeamError> {
    let state = &launcher.state;
    let run = state.team_run(lead).await?.ok_or(TeamError::NoTeam)?;
    if let TeamStatus::Ended(before) = run.status {
        return Err(TeamError::Ended(before));
    }
    if run.winding.is_some() {
        return end_for(launcher, lead, why.end(), kind, now).await;
    }
    let plan = state.plan(lead).await?;
    let at_work: Vec<TeamMember> = state
        .team_members(lead)
        .await?
        .into_iter()
        .filter(|member| matches!(member.status, TaskStatus::Claimed | TaskStatus::Running))
        .collect();
    let hands_over = |member: &TeamMember| {
        member.status == TaskStatus::Running
            && plan
                .iter()
                .any(|task| task.task_id == member.task_id && task.duty == TaskDuty::Work)
    };
    let mut returned = Vec::new();
    for member in at_work.iter().filter(|member| !hands_over(member)) {
        // Stored first, so the cancelled turn finds its task gone and counts no loss.
        let note = "the team stopped before it was done";
        match state
            .return_task(&member.thread_id, &member.task_id, note)
            .await
        {
            Ok(task) => returned.push(task),
            Err(error) => warn!("cannot return task {}: {error}", member.task_id),
        }
        cancel_member(launcher, &member.thread_id).await;
    }
    for task in &returned {
        events::task(launcher, lead, task).await;
    }
    // Stored even when nobody is left to wait for: the end line says why it stopped.
    state.wind_team(lead, why, now).await?;
    let asked = at_work.iter().filter(|member| hands_over(member)).count();
    if asked == 0 {
        let mut ended = end_for(launcher, lead, why.end(), kind, now).await?;
        returned.append(&mut ended);
        return Ok(returned);
    }
    let folder = dispatch::folder(launcher, lead).await?;
    for member in at_work.iter().filter(|member| hands_over(member)) {
        ask_to_wrap(launcher, &run, &member.thread_id, &folder).await;
    }
    events::team(launcher, lead, now, true).await;
    let cause = match why {
        WindWhy::Provider => format!(
            "The provider keeps failing{}; the team is stopping",
            provider_kind(kind)
        ),
        WindWhy::Stopped | WindWhy::Quiet => "Team winding down".to_owned(),
    };
    let line = match asked {
        1 => format!("{cause}: 1 member is writing its hand-over."),
        _ => format!("{cause}: {asked} members are writing their hand-over."),
    };
    let uid = format!("team:{lead}:{}:winding", run.started_at);
    lead_lines::tell(launcher, lead, lead, &uid, &line, false).await;
    Ok(returned)
}

/// A pass over a team that winds down: it ends once no member is at work, or the grace
/// ran out; until then each member at work is sure to have been asked, as after a
/// restart that came between the winding and the messages. The caller holds the pass
/// lock.
pub(super) async fn wound(
    launcher: &Launcher,
    run: &TeamRun,
    winding: Winding,
    folder: &Path,
    now: i64,
) -> Result<(), TeamError> {
    let lead = &run.thread_id;
    let state = &launcher.state;
    let grace = launcher.config.current().limits().team_wrap_seconds;
    let until = winding
        .since
        .saturating_add(i64::try_from(grace.saturating_mul(1000)).unwrap_or(i64::MAX));
    let at_work: Vec<TeamMember> = state
        .team_members(lead)
        .await?
        .into_iter()
        .filter(|member| matches!(member.status, TaskStatus::Claimed | TaskStatus::Running))
        .collect();
    if at_work.is_empty() || now >= until {
        end(launcher, lead, winding.why.end(), now).await?;
        return Ok(());
    }
    for member in &at_work {
        ask_to_wrap(launcher, run, &member.thread_id, folder).await;
    }
    Ok(())
}

/// The core's message that asks `member` to hand over, once for each run of the team.
async fn ask_to_wrap(launcher: &Launcher, run: &TeamRun, member: &str, folder: &Path) {
    let lead = &run.thread_id;
    let uid = format!("team:{lead}:{}:wrap:{member}", run.started_at);
    if let Err(error) = launcher
        .deliver_once(member, lead, &uid, WRAP, folder)
        .await
    {
        warn!("cannot ask member {member} to hand over: {error:#}");
    }
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
async fn tell(
    launcher: &Launcher,
    run: &TeamRun,
    why: TeamEnd,
    kind: Option<ProviderErrorKind>,
    now: i64,
) {
    let lead = &run.thread_id;
    let state = &launcher.state;
    let (Ok(plan), Ok(Some(team))) = (state.plan(lead).await, state.team(lead, now).await) else {
        return warn!("cannot read the ended team of thread {lead}");
    };
    let provider = run.winding.map(|winding| winding.why) == Some(WindWhy::Provider);
    let reason = match why {
        TeamEnd::Done => "done".to_owned(),
        // Only older cores ended a team on a budget.
        TeamEnd::Steps | TeamEnd::Tokens | TeamEnd::Time => "its budget was spent".to_owned(),
        TeamEnd::Stalled => "stalled, nothing could start".to_owned(),
        TeamEnd::Stopped if provider => format!(
            "stopped, the provider kept failing the members' turns{}",
            provider_kind(kind)
        ),
        TeamEnd::Stopped => "stopped".to_owned(),
        TeamEnd::Quiet => "quiet, you were written to 30 times without the user".to_owned(),
    };
    let left = match why {
        TeamEnd::Done => "",
        _ => " Tasks at work went back to open; run goes on from here.",
    };
    // The lead's own tasks; reviews and the critic are the core's.
    let work: Vec<&PlanTask> = plan
        .iter()
        .filter(|task| task.duty == TaskDuty::Work)
        .collect();
    let done = work
        .iter()
        .filter(|task| task.status == PlanStatus::Done)
        .count();
    let skipped = match work.iter().filter(|task| task.skipped).count() {
        0 => String::new(),
        count => format!(", {count} skipped"),
    };
    let critic = critic::end_words(&plan, run);
    let line = format!(
        "Team ended: {reason}. {done} of {} tasks{skipped}; {} steps, {} tokens, {} min.{left}\
         {critic}",
        work.len(),
        team.used.steps,
        tokens(team.used.tokens),
        team.used.minutes
    );
    let uid = format!("team:{lead}:{}:end", run.started_at);
    let wake = why != TeamEnd::Stopped;
    lead_lines::tell(launcher, lead, lead, &uid, &line, wake).await;
}

/// `wave 2 waits for wave 1: #a failed, #b blocked`: what holds the next wave, when the
/// current wave's unfinished tasks are failed or blocked and a later wave waits. Empty
/// otherwise.
fn held_wave(plan: &[PlanTask]) -> String {
    let Some(current) = current_wave(plan) else {
        return String::new();
    };
    let waves = waves(plan);
    let later = plan
        .iter()
        .zip(&waves)
        .any(|(task, wave)| *wave > current && !task.finished());
    let holding: Vec<String> = plan
        .iter()
        .zip(&waves)
        .filter(|(task, wave)| **wave == current && !task.finished())
        .filter(|(task, _)| matches!(task.status, PlanStatus::Failed | PlanStatus::Blocked))
        .take(4)
        .map(|(task, _)| format!("#{} {}", short_task_id(&task.task_id), task.status.as_str()))
        .collect();
    if !later || holding.is_empty() {
        return String::new();
    }
    format!(
        "wave {} waits for wave {}: {}",
        current + 2,
        current + 1,
        holding.join(", ")
    )
}

/// ` (rate limited)`: the provider's reason, when it gave one.
fn provider_kind(kind: Option<ProviderErrorKind>) -> String {
    kind.map(|kind| format!(" ({})", kind.label().to_lowercase()))
        .unwrap_or_default()
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
    if counts.total > 0 && counts.done + counts.skipped == counts.total {
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
            let plan = state.plan(lead).await?;
            let line = lead_lines::stall_line(&counts, &held_wave(&plan));
            lead_lines::tell(launcher, lead, lead, &stall, &line, true).await;
        }
    }
    Ok(())
}
