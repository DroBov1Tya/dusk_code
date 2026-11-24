//! The command gate of a handed-in task: the core runs it, with no model call. It runs in
//! the team's folder as a member's own command would: approved under the chat's approval
//! mode, which a member's is a copy of, and confined by it. Exit 0 passes the task.
//! Anything else sends it back to its member with the end of the output, at most
//! `MAX_REWORKS` times; after that the task fails and the lead decides.
//!
//! A gate runs `limits.team_gate_seconds` at most, a team runs `limits.team_max_gate_runs`
//! of them, and the end of the team kills the ones that run (`stop`).
//!
//! Nothing of a running gate is stored. A gate is a check, so one that a stop of the core
//! or an end of the team cut off simply runs again: a pass starts the gate of every task
//! that is in review for a command and has none running (`resume`).

use std::{future::Future, pin::Pin, sync::Arc};

use anyhow::{Context, Result};
use dusk_protocol::thread::TeamStatus;
use tokio::task::AbortHandle;
use tracing::{info, warn};

use super::{allow, clean, dispatch, events, lead_lines, review};
use crate::{
    message::plain::plain,
    runtime::{Launcher, thread::start},
    state::{GoalCheck, PassedBy, PlanError, PlanStatus, PlanTask, timestamp_ms},
    tools::{ToolContext, ToolResult, console::run_check},
};

/// How the note of a task begins when its gate sent it back; `member::deliver_rework`
/// frames such a note as program output.
pub(super) const FAILED: &str = "gate failed";
/// Lines of the gate's output a member reads, and their most characters together. The
/// note's column takes 600 with what stands before them.
const TAIL_LINES: usize = 20;
const TAIL_CHARS: usize = 480;
/// What the lead reads of a task whose answer is not at hand, as after a restart.
const NO_RESULT: &str = "see its thread";

/// A gate whose command runs now, so a pass starts none twice and an end kills it.
pub(super) struct Running {
    lead: String,
    /// Set once the run is spawned; aborting it drops the command, which kills its
    /// process group.
    abort: Option<AbortHandle>,
}

/// Begins the gate of `task`, which its member handed in with `result`, unless it runs
/// already. Answers with the task as it is now: still in review while the gate runs, or
/// held for the lead when the command may not run unasked or the team's gate runs are
/// spent.
pub(super) async fn begin(
    launcher: &Launcher,
    lead: &str,
    member: &str,
    task: &PlanTask,
    result: &str,
) -> Result<PlanTask> {
    let Some(GoalCheck::Command { command }) = &task.gate else {
        return Ok(task.clone());
    };
    {
        let mut gates = launcher.team.gates.lock();
        if gates.contains_key(&task.task_id) {
            return Ok(task.clone());
        }
        let running = Running {
            lead: lead.to_owned(),
            abort: None,
        };
        gates.insert(task.task_id.clone(), running);
    }
    let context = match approved(launcher, lead, command).await {
        Ok(context) => context,
        Err(error) => {
            launcher.team.gates.lock().remove(&task.task_id);
            // Deny by default: nobody can be asked from here, so the lead decides.
            let why = format!(
                "its gate was not run: {}",
                lead_lines::quoted(&format!("{error:#}"), 200)
            );
            return hold(launcher, lead, member, task, result, &why).await;
        }
    };
    let limits = context.config.limits();
    if !count_run(launcher, lead, limits.team_max_gate_runs) {
        launcher.team.gates.lock().remove(&task.task_id);
        let why = format!(
            "its gate was not run: the team ran its {} gates",
            limits.team_max_gate_runs
        );
        return hold(launcher, lead, member, task, result, &why).await;
    }
    let spawned = tokio::spawn(run(
        launcher.clone(),
        lead.to_owned(),
        task.task_id.clone(),
        command.clone(),
        context,
    ));
    match launcher.team.gates.lock().get_mut(&task.task_id) {
        Some(running) => running.abort = Some(spawned.abort_handle()),
        // The team ended between the two: nothing is to run for it.
        None => spawned.abort(),
    }
    Ok(task.clone())
}

/// Holds the task for the lead, who reads `why`.
async fn hold(
    launcher: &Launcher,
    lead: &str,
    member: &str,
    task: &PlanTask,
    result: &str,
    why: &str,
) -> Result<PlanTask> {
    let why = clean::note(launcher, lead, why).await;
    let held = launcher
        .state
        .hold_for_lead(lead, &task.task_id, &why)
        .await?;
    lead_lines::review(launcher, lead, member, &held, result, &why).await;
    Ok(held)
}

/// Counts one more gate run of the team; false once `cap` are spent.
fn count_run(launcher: &Launcher, lead: &str, cap: u32) -> bool {
    let mut runs = launcher.team.gate_runs.lock();
    let ran = runs.entry(lead.to_owned()).or_default();
    if *ran >= cap {
        return false;
    }
    *ran += 1;
    true
}

/// The team of `lead` ended: its gates that run now are killed, and its count of gate
/// runs starts again with the next team. Their tasks stay in review, and the next run
/// runs their gates again (`resume`): a gate is a check.
pub(super) fn stop(launcher: &Launcher, lead: &str) {
    let stopped: Vec<Running> = {
        let mut gates = launcher.team.gates.lock();
        let ids: Vec<String> = gates
            .iter()
            .filter(|(_, running)| running.lead == lead)
            .map(|(id, _)| id.clone())
            .collect();
        ids.iter().filter_map(|id| gates.remove(id)).collect()
    };
    for running in stopped {
        if let Some(abort) = running.abort {
            abort.abort();
        }
    }
    launcher.team.gate_runs.lock().remove(lead);
}

/// Runs the command and moves the task by how it ended. Boxed, since the pass that
/// follows may begin a gate again.
fn run(
    launcher: Launcher,
    lead: String,
    task_id: String,
    command: String,
    context: ToolContext,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let seconds = context.config.limits().team_gate_seconds;
        let ran = run_check(&command, &context, seconds).await;
        // An end of the team took the gate off the list: it is not the team's anymore.
        if launcher.team.gates.lock().remove(&task_id).is_none() {
            return;
        }
        // A stop of the core killed the command: its result says nothing about the task.
        if launcher.mailboxes.is_closed() {
            return;
        }
        match settle(&launcher, &lead, &task_id, ran).await {
            Ok(()) => launcher.team_changed(&lead).await,
            Err(error) => warn!("gate of task {task_id}: {error:#}"),
        }
    })
}

/// Starts the gate of every task of the plan that waits for one and has none running:
/// what a stop of the core, or a team that ended meanwhile, left in review.
pub(super) async fn resume(launcher: &Launcher, lead: &str) -> Result<()> {
    for task in launcher.state.plan(lead).await? {
        // A task held for the lead, or whose review runs, passed its gate or needs none.
        let waits = task.status == PlanStatus::Review
            && matches!(task.gate, Some(GoalCheck::Command { .. }))
            && task.waits_on.is_none();
        let Some(member) = task.owner_thread_id.as_deref().filter(|_| waits) else {
            continue;
        };
        let task = begin(launcher, lead, member, &task, NO_RESULT).await?;
        events::task(launcher, lead, &task).await;
    }
    Ok(())
}

/// The command's approval and where it runs. It is judged by the chat's approval mode
/// as it is now, which is what a member's own is copied from: a mode the user tightened
/// while the member worked holds for its gate. Nobody listens for a question here, so
/// a mode that would ask refuses instead, unless the user allowed this very line for
/// the team when `run` asked (`allow`). Confinement is the mode's either way.
async fn approved(launcher: &Launcher, lead: &str, command: &str) -> Result<ToolContext> {
    let state = &launcher.state;
    let folder = dispatch::folder(launcher, lead).await?;
    let workspace = start::workspace(state, &launcher.paths, lead).await?;
    let context = ToolContext {
        scope: Arc::new(launcher.scope(&folder, &workspace)?),
        config: launcher.config.current(),
        progress: None,
        approvals: Some(Arc::clone(&launcher.approvals)),
        state: Some(Arc::clone(state)),
        thread_id: lead.to_owned(),
    };
    if !allow::covers(launcher, lead, command) {
        context
            .approve_command(command)
            .await
            .context("a gate cannot ask for approval; run asks once for the plan's gates")?;
    }
    Ok(context)
}

/// What the gate's run does to its task. Only a task still in review is moved: the lead
/// may have failed it meanwhile.
async fn settle(
    launcher: &Launcher,
    lead: &str,
    task_id: &str,
    ran: Result<ToolResult>,
) -> Result<()> {
    let state = &launcher.state;
    let _one = launcher.team.pass.lock().await;
    let note = match &ran {
        Ok(result) if result.exit_code == Some(0) => None,
        Ok(result) => Some(failure(result)),
        Err(error) => Some(format!(
            "{FAILED}: {}",
            lead_lines::quoted(&format!("{error:#}"), 300)
        )),
    };
    let moved = match &note {
        None => {
            // A passed gate is checked once more, by a fresh reviewer, when the task
            // changed files.
            let task = state.plan_task(lead, task_id).await?;
            let member = task.owner_thread_id.clone().unwrap_or_default();
            let reviewed = task.status == PlanStatus::Review
                && task.waits_on.is_none()
                && review::due(launcher, &member, &task).await?;
            match reviewed {
                true => review::begin(launcher, lead, &task)
                    .await
                    .map_err(|error| PlanError::Store(error.context("cannot begin the review"))),
                false => state.pass_task(lead, task_id, PassedBy::Gate).await,
            }
        }
        Some(note) => {
            // A team that ended gives no task back to a member: the gate runs again
            // with the next run.
            let running = state
                .team_run(lead)
                .await?
                .is_some_and(|run| run.status == TeamStatus::Running);
            if !running {
                return Ok(());
            }
            // A build prints what it was given: tokens, URLs with a password.
            let note = clean::note(launcher, lead, note).await;
            state
                .rework_task(lead, task_id, &note, timestamp_ms()?)
                .await
        }
    };
    let task = match moved {
        Ok(task) => task,
        Err(PlanError::Illegal { .. } | PlanError::Gate { .. }) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    info!(
        "gate of task {task_id}: the task is {}",
        task.status.as_str()
    );
    // A task that came back too often failed: the lead reads why. One that goes back
    // to its member is woken by the pass that follows.
    dispatch::released(launcher, lead, &task).await;
    Ok(())
}

/// `gate failed, exit 101: <the end of its output>`, on one line: the note of the task,
/// which its member reads as program output and the lead in a failed line.
fn failure(result: &ToolResult) -> String {
    let exit = match result.exit_code {
        Some(code) => format!("exit {code}"),
        None => "ended by a signal".into(),
    };
    // The first line repeats the exit code.
    let output = result.content.split_once('\n').map_or("", |(_, rest)| rest);
    format!("{FAILED}, {exit}: {}", tail(output))
}

/// The last lines of `output`, each made plain, joined by ` | ` and cut from the front
/// to `TAIL_CHARS`: the end of a build's output is where its errors are.
fn tail(output: &str) -> String {
    let mut lines: Vec<String> = output
        .lines()
        .rev()
        .map(plain)
        .filter(|line| !line.is_empty())
        .take(TAIL_LINES)
        .collect();
    lines.reverse();
    let joined = lines.join(" | ");
    let skip = joined.chars().count().saturating_sub(TAIL_CHARS);
    joined.chars().skip(skip).collect()
}
