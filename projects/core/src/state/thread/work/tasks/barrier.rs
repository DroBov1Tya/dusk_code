//! The wave barrier: a team gives out one wave at a time. A task's wave is one past the
//! deepest of the tasks it needs, as every client draws it (`team_layout::depths_of`), so
//! what the user sees as a wave is what the core waits on. The current wave is the lowest
//! that still holds a task not finished (done, or skipped by the lead); a task of a later
//! wave waits for it even when its own dependencies are done. A failed, blocked or
//! reviewed task of the current wave therefore holds the next one until it is done or
//! the lead reopens, fixes or skips it.

use dusk_protocol::thread::{TaskDuty, team_layout::depths_of};

use super::plan::PlanTask;

/// The wave of each task of `plan`, in its order.
pub fn waves(plan: &[PlanTask]) -> Vec<usize> {
    let nodes: Vec<(&str, &[String], TaskDuty)> = plan
        .iter()
        .map(|task| (task.task_id.as_str(), task.depends_on.as_slice(), task.duty))
        .collect();
    depths_of(&nodes)
}

/// The lowest wave that holds a task not finished; `None` when every task is.
pub fn current_wave(plan: &[PlanTask]) -> Option<usize> {
    let waves = waves(plan);
    plan.iter()
        .zip(waves)
        .filter(|(task, _)| !task.finished())
        .map(|(_, wave)| wave)
        .min()
}
