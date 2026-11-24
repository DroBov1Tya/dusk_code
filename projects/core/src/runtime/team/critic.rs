//! The completeness check at the end of a run: once every task of the lead's plan and
//! every review is done or skipped, one fresh member compares the results with what the
//! user asked and lists what no task did and what no check verified. Its answer is read
//! by the end line, which hands the list to the lead. One critic a run; a run the lead
//! starts again after it gets its own.

use anyhow::Result;
use dusk_protocol::thread::TaskDuty;

use super::{card, events};
use crate::{
    runtime::Launcher,
    state::{Duty, PlanStatus, PlanTask, TeamRun, short_task_id},
};

/// What the critic's row is called on the plan.
const TITLE: &str = "Completeness check";

/// The critic of `run`, if one was added.
pub(super) fn of_run<'a>(plan: &'a [PlanTask], run: &TeamRun) -> Option<&'a PlanTask> {
    plan.iter()
        .filter(|task| task.duty == TaskDuty::Critic && task.created_at >= run.started_at)
        .max_by_key(|task| task.created_at)
}

/// Adds the critic of the run when it is due: checks are on, no critic was added for this
/// run, every other row is finished, and some task of the lead's was done.
pub(super) async fn add_if_due(launcher: &Launcher, run: &TeamRun) -> Result<()> {
    let config = launcher.config.current();
    if !config.limits().team_review {
        return Ok(());
    }
    let lead = &run.thread_id;
    let state = &launcher.state;
    let plan = state.plan(lead).await?;
    if of_run(&plan, run).is_some() {
        return Ok(());
    }
    let others = plan.iter().filter(|task| task.duty != TaskDuty::Critic);
    let mut any_done = false;
    for task in others {
        if !task.finished() {
            return Ok(());
        }
        any_done |= task.duty == TaskDuty::Work && task.status == PlanStatus::Done;
    }
    if !any_done {
        return Ok(());
    }
    let duty = Duty {
        duty: TaskDuty::Critic,
        role: config.team().critic_role(),
        description: TITLE,
    };
    let row = state.add_critic(lead, duty).await?;
    events::task(launcher, lead, &row).await;
    Ok(())
}

/// The critic's card: the goal of the run, the user's last request, the results.
pub(super) async fn card(
    launcher: &Launcher,
    lead: &str,
    row: &PlanTask,
    plan: &[PlanTask],
) -> Result<String> {
    let state = &launcher.state;
    let goal = state
        .team_run(lead)
        .await?
        .map(|run| run.goal)
        .unwrap_or_default();
    let request = state.last_user_text(lead).await?.unwrap_or_default();
    Ok(card::critic_card(row, &goal, &request, plan))
}

/// The end line's words on the critic of `run`, empty when it had none.
pub(super) fn end_words(plan: &[PlanTask], run: &TeamRun) -> String {
    let Some(critic) = of_run(plan, run) else {
        return String::new();
    };
    if critic.status != PlanStatus::Done {
        return " The critic did not finish; nothing checked the results as a whole.".into();
    }
    let count = |label: &str| {
        critic
            .result
            .lines()
            .filter(|line| line.starts_with(label))
            .count()
    };
    let (missing, unverified) = (count("Missing:"), count("Unverified:"));
    let id = short_task_id(&critic.task_id);
    match (missing, unverified) {
        (0, 0) => format!(" Critic: nothing missing or unverified (#{id})."),
        _ => format!(
            " Critic: {missing} missing, {unverified} unverified; show #{id}. Add tasks and \
             run, or report them as open."
        ),
    }
}
