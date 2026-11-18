//! One pass of the dispatcher over a chat's team: everything the core does by itself for
//! a plan, read from the tables each time, so a pass after a restart goes on where the
//! last one stopped. In order: the lead that went quiet ends the team; leases past their
//! time are released; members whose card or rework note did not arrive get it; a gate
//! that a stop cut off runs again; the
//! budget is checked and ready tasks are given out, lowest phase label first and oldest
//! first within it; then the end and the stall rule.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use dusk_protocol::thread::{TaskStatus, TeamEnd, TeamStatus};
use tracing::warn;

use super::{
    clean, end, events, gate, lead_lines, lease,
    member::{self, Given},
};
use crate::{
    runtime::{Launcher, agent::MAX_AGENT_MESSAGES},
    state::{PlanStatus, PlanTask, TeamAsk, TeamCheck, TeamRun, WaitsOn},
};

/// Runs one pass at `now`; answers how many tasks it gave out. Nothing happens for a
/// chat without a running team, or with the switch off.
pub(super) async fn pass(launcher: &Launcher, lead: &str, now: i64) -> Result<usize> {
    if !launcher.config.current().limits().plan_graph {
        return Ok(0);
    }
    let _one = launcher.team.pass.lock().await;
    let state = &launcher.state;
    let Some(run) = state.team_run(lead).await? else {
        return Ok(0);
    };
    if run.status != TeamStatus::Running {
        return Ok(0);
    }
    // Past the cap the lead is not woken by a line any more: it could not review, and
    // members would work on with nobody reading.
    if state.agent_messages_since_client(lead).await? >= MAX_AGENT_MESSAGES {
        end::end(launcher, lead, TeamEnd::Quiet, now).await?;
        return Ok(0);
    }
    let folder = folder(launcher, lead).await?;
    expire(launcher, lead, now).await?;
    wake_members(launcher, lead, &folder).await?;
    gate::resume(launcher, lead).await?;
    let Some(given) = give_out(launcher, &run, &folder, now).await? else {
        return Ok(0);
    };
    end::settle(launcher, &run, now).await?;
    Ok(given)
}

/// The folder members work in: the lead's project folder.
pub(super) async fn folder(launcher: &Launcher, lead: &str) -> Result<PathBuf> {
    let chat = launcher.state.load_thread(lead).await?;
    chat.and_then(|chat| chat.project)
        .map(PathBuf::from)
        .with_context(|| format!("thread {lead} has no project folder for its team"))
}

/// Releases the tasks whose lease ran out before `now`, and stops what is left of their
/// members: a runner that lives without its heartbeat, a card nobody read.
async fn expire(launcher: &Launcher, lead: &str, now: i64) -> Result<()> {
    let state = &launcher.state;
    let members = state.team_members(lead).await?;
    for task in state.release_expired(lead, now).await? {
        if let Some(member) = members.iter().find(|held| held.task_id == task.task_id) {
            end::cancel_member(launcher, &member.thread_id).await;
        }
        released(launcher, lead, &task).await;
    }
    Ok(())
}

/// A member's turn was lost: its task goes back to open, or fails at the third time.
/// Only a task the member holds at work is released; the pass lock keeps a claim from
/// coming between the look and the release. Answers whether a task moved.
pub(super) async fn lost(
    launcher: &Launcher,
    lead: &str,
    member: &str,
    note: &str,
) -> Result<bool> {
    let state = &launcher.state;
    let task = {
        let _one = launcher.team.pass.lock().await;
        let Some(held) = lease::at_work(state, lead, member).await? else {
            return Ok(false);
        };
        let note = clean::note(launcher, lead, note).await;
        state.release_task(lead, &held.task_id, &note).await?
    };
    released(launcher, lead, &task).await;
    Ok(true)
}

/// After a release: clients hear of the task, and when it failed, of the tasks the
/// store blocked behind it, and the lead gets its one line.
pub(super) async fn released(launcher: &Launcher, lead: &str, task: &PlanTask) {
    events::task(launcher, lead, task).await;
    if task.status != PlanStatus::Failed {
        return;
    }
    lead_lines::failed(launcher, lead, task).await;
    let behind = WaitsOn::Task(task.task_id.clone());
    match launcher.state.plan(lead).await {
        Ok(plan) => {
            for blocked in plan.iter().filter(|other| {
                other.status == PlanStatus::Blocked && other.waits_on.as_ref() == Some(&behind)
            }) {
                events::task(launcher, lead, blocked).await;
            }
        }
        Err(error) => warn!("cannot read the plan of thread {lead}: {error}"),
    }
}

/// Members at work get what they were to be woken with, if a stop came between: the card
/// of a claimed task, the note of a task the lead sent back. Both are stored once under
/// their uid, so this costs a look when nothing is missing.
async fn wake_members(launcher: &Launcher, lead: &str, folder: &Path) -> Result<()> {
    let state = &launcher.state;
    for held in state.team_members(lead).await? {
        let member = &held.thread_id;
        match held.status {
            TaskStatus::Claimed => {
                let task = state.plan_task(lead, &held.task_id).await?;
                member::deliver_card(launcher, lead, member, &task, folder).await?;
            }
            TaskStatus::Running => {
                let task = state.plan_task(lead, &held.task_id).await?;
                if task.reworks > 0 {
                    member::deliver_rework(launcher, lead, member, &task, folder).await?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Gives out ready tasks while the budget and the members at once allow. `None` when
/// the team ended instead.
async fn give_out(
    launcher: &Launcher,
    run: &TeamRun,
    folder: &Path,
    now: i64,
) -> Result<Option<usize>> {
    let lead = &run.thread_id;
    let state = &launcher.state;
    let mut given = 0;
    // Tasks this pass could not give out; the next pass tries them again.
    let mut passed_over: Vec<String> = Vec::new();
    loop {
        match state.team_check(lead, TeamAsk::Claim, now).await? {
            TeamCheck::Go => {}
            TeamCheck::Full => break,
            TeamCheck::Spent(spent) => {
                end::end(launcher, lead, spent.end(), now).await?;
                return Ok(None);
            }
            TeamCheck::Ended(_) => return Ok(None),
        }
        // The store orders them by phase label, then age.
        let ready = state.ready_tasks(lead).await?;
        let Some(task) = ready
            .into_iter()
            .find(|task| !passed_over.contains(&task.task_id))
        else {
            break;
        };
        match member::give(launcher, lead, run, &task, folder, now).await? {
            Given::Claimed(claimed) => {
                given += 1;
                events::task(launcher, lead, &claimed).await;
            }
            Given::Lost => passed_over.push(task.task_id),
            Given::NoSlot => break,
            Given::NoAgent(why) => {
                let why = clean::note(launcher, lead, &lead_lines::quoted(&why, 300)).await;
                let failed = state.fail_task(lead, &task.task_id, &why).await?;
                released(launcher, lead, &failed).await;
            }
        }
    }
    if given > 0 {
        events::team(launcher, lead, now, true).await;
    }
    Ok(Some(given))
}
