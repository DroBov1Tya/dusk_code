//! One pass of the dispatcher over a chat's team: everything the core does by itself for
//! a plan, read from the tables each time, so a pass after a restart goes on where the
//! last one stopped. A team that winds down only waits for its members' hand-overs and
//! ends (`end::wound`). Otherwise, in order: the lead that went quiet winds the team
//! down; leases past their
//! time are released; members whose card or rework note did not arrive get it; a gate
//! that a stop cut off runs again, and a task whose review row is lost waits for the
//! lead; the critic is added once every other task is finished; ready tasks of the
//! current wave are given out while members at once allow, lowest phase label first and
//! oldest first within it; then the end and the stall rule.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use dusk_protocol::{
    model::ProviderErrorKind,
    thread::{TaskDuty, TaskStatus, TeamStatus},
};
use tracing::{info, warn};

use super::{
    clean, critic, end, events, gate, lead_lines, lease,
    member::{self, Given},
    review,
};
use crate::{
    runtime::{Launcher, agent::MAX_AGENT_MESSAGES},
    state::{PlanStatus, PlanTask, TeamAsk, TeamCheck, TeamRun, WaitsOn, WindWhy},
};

/// Provider failures in a row, across the team's members, that wind it down.
const MAX_INFRA_LOSSES: u32 = 3;

/// Why a member's turn was lost, which decides whether the loss counts against its
/// task's attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Loss {
    /// The member's own: its turn failed on what it did, or on a request the provider
    /// refused. Counted; the third fails the task.
    Member,
    /// Cut off from outside: a restart, or a team that winds down. Not counted.
    Cut,
    /// The provider or the runner failed under it, with the provider's reason when it
    /// gave one. Not counted; `MAX_INFRA_LOSSES` in a row, or a key or quota the
    /// provider refuses, wind the team down.
    Infra(Option<ProviderErrorKind>),
}

impl Loss {
    /// A turn that failed: the provider's fault, or the member's.
    pub fn of_failure(kind: Option<ProviderErrorKind>) -> Self {
        match kind {
            Some(kind) if kind.is_providers_fault() => Self::Infra(Some(kind)),
            _ => Self::Member,
        }
    }
}

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
    let folder = folder(launcher, lead).await?;
    if let Some(winding) = run.winding {
        expire(launcher, lead, now).await?;
        wake_members(launcher, lead, &folder).await?;
        end::wound(launcher, &run, winding, &folder, now).await?;
        return Ok(0);
    }
    // Past the cap the lead is not woken by a line any more: it could not review, and
    // members would work on with nobody reading.
    if state.agent_messages_since_client(lead).await? >= MAX_AGENT_MESSAGES {
        end::wind(launcher, lead, WindWhy::Quiet, now).await?;
        return Ok(0);
    }
    expire(launcher, lead, now).await?;
    wake_members(launcher, lead, &folder).await?;
    gate::resume(launcher, lead).await?;
    review::resume(launcher, lead).await?;
    // A team is led only while its chat is in TEAM mode. In another mode the lead has
    // no plan tool to answer with, so nothing new is given out and the stall rule does
    // not wake it; what is at work finishes, and the team goes on when the mode is back.
    if !super::in_team_mode(launcher, lead).await? {
        return Ok(0);
    }
    critic::add_if_due(launcher, &run).await?;
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

/// A member's turn was lost: its task goes back to open, or, for a loss of the member's
/// own, fails at the third time. A loss while the team winds down is never counted: the
/// core told the member to stop. Only a task the member holds at work is released; the
/// pass lock keeps a claim from coming between the look and the release. Answers
/// whether a task moved.
pub(super) async fn lost(
    launcher: &Launcher,
    lead: &str,
    member: &str,
    note: &str,
    loss: Loss,
) -> Result<bool> {
    let state = &launcher.state;
    let task = {
        let _one = launcher.team.pass.lock().await;
        let Some(held) = lease::at_work(state, lead, member).await? else {
            return Ok(false);
        };
        let winding = state
            .team_run(lead)
            .await?
            .is_some_and(|run| run.winding.is_some());
        let loss = if winding { Loss::Cut } else { loss };
        let note = clean::note(launcher, lead, note).await;
        let task = match loss {
            Loss::Member => state.release_task(lead, &held.task_id, &note).await?,
            Loss::Cut | Loss::Infra(_) => state.return_task(member, &held.task_id, &note).await?,
        };
        if let Loss::Infra(kind) = loss {
            provider_failed(launcher, lead, kind).await;
        }
        task
    };
    released(launcher, lead, &task).await;
    Ok(true)
}

/// Counts a loss the provider or the runner caused. A key or a quota the provider
/// refuses winds the team down at once, as do `MAX_INFRA_LOSSES` in a row: every
/// member would fail the same way, and none of it would count. A hand-in ends the row
/// (`infra_passed`). The caller holds the pass lock.
async fn provider_failed(launcher: &Launcher, lead: &str, kind: Option<ProviderErrorKind>) {
    let in_row = {
        let mut losses = launcher.team.infra_losses.lock();
        let count = losses.entry(lead.to_owned()).or_default();
        *count += 1;
        *count
    };
    let refused = matches!(
        kind,
        Some(ProviderErrorKind::Auth | ProviderErrorKind::Quota)
    );
    if !refused && in_row < MAX_INFRA_LOSSES {
        return;
    }
    info!("team of thread {lead}: the provider keeps failing ({kind:?}); it winds down");
    launcher.team.infra_losses.lock().remove(lead);
    let wound = match crate::state::timestamp_ms() {
        Ok(now) => end::wind(launcher, lead, WindWhy::Provider, now)
            .await
            .map(drop)
            .map_err(anyhow::Error::from),
        Err(error) => Err(error),
    };
    if let Err(error) = wound {
        warn!("team of thread {lead}: cannot wind it down: {error:#}");
    }
}

/// A member handed in: the provider answered, and losses in a row start again.
pub(super) fn infra_passed(launcher: &Launcher, lead: &str) {
    launcher.team.infra_losses.lock().remove(lead);
}

/// After a release: clients hear of the task, and when it failed, of the tasks the
/// store blocked behind it, and the lead gets its one line. A review or the critic that
/// failed is given up instead: it never fails the work it checks.
pub(super) async fn released(launcher: &Launcher, lead: &str, task: &PlanTask) {
    events::task(launcher, lead, task).await;
    if task.status != PlanStatus::Failed {
        return;
    }
    let given_up = match task.duty {
        TaskDuty::Work => return failed(launcher, lead, task).await,
        TaskDuty::Review => review::failed(launcher, lead, task).await,
        TaskDuty::Critic => skip(launcher, lead, task).await,
    };
    if let Err(error) = given_up {
        warn!(
            "team of thread {lead}: cannot give up task {}: {error:#}",
            task.task_id
        );
    }
}

async fn skip(launcher: &Launcher, lead: &str, task: &PlanTask) -> Result<()> {
    for skipped in launcher
        .state
        .skip_task(lead, &task.task_id, &task.note)
        .await?
    {
        events::task(launcher, lead, &skipped).await;
    }
    Ok(())
}

/// A task of the lead's plan failed: its line, and the tasks blocked behind it.
async fn failed(launcher: &Launcher, lead: &str, task: &PlanTask) {
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

/// Gives out ready tasks while the members at once allow. `None` when
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
        match state.team_check(lead, TeamAsk::Claim).await? {
            TeamCheck::Go => {}
            TeamCheck::Full | TeamCheck::Winding(_) => break,
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
