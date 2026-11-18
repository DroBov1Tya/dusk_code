//! Where the gate of a handed-in task starts. T2 fills `begin`: it runs the command as a
//! background job of the member's thread and passes the task or sends it back. Until
//! then no command is run here, and the task waits for the lead, who is told why.

use anyhow::Result;

use super::lead_lines;
use crate::{
    runtime::Launcher,
    state::{PlanTask, Thread},
};

const NOT_RUN: &str = "its gate was not run: the core does not run gates yet";

/// Begins the command gate of `task`, which `member` just handed in with `result`.
/// Answers with the task as it is afterwards.
pub(super) async fn begin(
    launcher: &Launcher,
    lead: &str,
    member: &Thread,
    task: &PlanTask,
    result: &str,
) -> Result<PlanTask> {
    let held = launcher
        .state
        .hold_for_lead(lead, &task.task_id, NOT_RUN)
        .await?;
    lead_lines::review(launcher, lead, member, &held, result, NOT_RUN).await;
    Ok(held)
}
