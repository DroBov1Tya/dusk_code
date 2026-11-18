//! The command gate of a handed-in task: the core runs it, with no model call. It runs in
//! the team's folder as the member's own command would: approved under the member's
//! approval mode and confined by it. Exit 0 passes the task. Anything else sends it back
//! to its member with the end of the output, at most `MAX_REWORKS` times; after that the
//! task fails and the lead decides.
//!
//! Nothing of a running gate is stored. A gate is a check, so one that a stop of the core
//! cut off simply runs again: a pass starts the gate of every task that is in review for
//! a command and has none running (`resume`).

use std::{future::Future, pin::Pin, sync::Arc};

use anyhow::{Context, Result};
use dusk_protocol::thread::TeamStatus;
use tracing::{info, warn};

use super::{dispatch, events, lead_lines};
use crate::{
    message::plain::plain,
    runtime::{Launcher, thread::start},
    state::{GoalCheck, PassedBy, PlanError, PlanStatus, PlanTask, WaitsOn, timestamp_ms},
    tools::{
        ToolContext, ToolResult,
        console::{CHECK_TIMEOUT_SECONDS, run_check},
    },
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

/// Begins the gate of `task`, which its member handed in with `result`, unless it runs
/// already. Answers with the task as it is now: still in review while the gate runs, or
/// held for the lead when the command may not run unasked.
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
    if !launcher.team.gates.lock().insert(task.task_id.clone()) {
        return Ok(task.clone());
    }
    let context = match approved(launcher, lead, member, command).await {
        Ok(context) => context,
        Err(error) => {
            launcher.team.gates.lock().remove(&task.task_id);
            // Deny by default: nobody can be asked from here, so the lead decides.
            let why = format!(
                "its gate was not run: {}",
                lead_lines::quoted(&format!("{error:#}"), 200)
            );
            let held = launcher
                .state
                .hold_for_lead(lead, &task.task_id, &why)
                .await?;
            lead_lines::review(launcher, lead, member, &held, result, &why).await;
            return Ok(held);
        }
    };
    tokio::spawn(run(
        launcher.clone(),
        lead.to_owned(),
        task.task_id.clone(),
        command.clone(),
        context,
    ));
    Ok(task.clone())
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
        let ran = run_check(&command, &context, CHECK_TIMEOUT_SECONDS).await;
        launcher.team.gates.lock().remove(&task_id);
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
        let waits = task.status == PlanStatus::Review
            && matches!(task.gate, Some(GoalCheck::Command { .. }))
            && task.waits_on != Some(WaitsOn::Lead);
        let Some(member) = task.owner_thread_id.as_deref().filter(|_| waits) else {
            continue;
        };
        let task = begin(launcher, lead, member, &task, NO_RESULT).await?;
        events::task(launcher, lead, &task).await;
    }
    Ok(())
}

/// The command's approval under the member's own mode, and where it runs. Nobody
/// listens for a question here, so a mode that would ask refuses instead.
async fn approved(
    launcher: &Launcher,
    lead: &str,
    member: &str,
    command: &str,
) -> Result<ToolContext> {
    let state = &launcher.state;
    let folder = dispatch::folder(launcher, lead).await?;
    let workspace = start::workspace(state, &launcher.paths, member).await?;
    let context = ToolContext {
        scope: Arc::new(launcher.scope(&folder, &workspace)?),
        config: launcher.config.current(),
        progress: None,
        approvals: Some(Arc::clone(&launcher.approvals)),
        state: Some(Arc::clone(state)),
        thread_id: member.to_owned(),
        graph: None,
    };
    context
        .approve_command(command)
        .await
        .context("a gate cannot ask for approval")?;
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
        None => state.pass_task(lead, task_id, PassedBy::Gate).await,
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
            state
                .rework_task(lead, task_id, note, timestamp_ms()?)
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
