//! A chat's plan and its team: what a client reads of them, and the few changes a user
//! makes by hand. Every call names a thread of the chat and finds the plan from it, so a
//! task of another chat is never reached. The refusals are the store's own words; a
//! failure of the store itself is logged here and answered without its text.

use anyhow::{Result, anyhow};
use dusk_protocol::thread::{PlanPage, Task, Team, TeamBudget};
use tracing::warn;

use crate::{
    runtime::Runtime,
    state::{BudgetBy, PassedBy, PlanError, TeamCeilings, TeamError, timestamp_ms},
};

impl Runtime {
    /// A page of the chat's plan for the strip; see `State::plan_page`.
    pub async fn plan_page(
        &self,
        thread_id: &str,
        done_before: Option<&str>,
        limit: u32,
    ) -> Result<PlanPage> {
        let state = self.state_of(thread_id).await;
        state
            .plan_page(thread_id, done_before, limit)
            .await
            .map_err(plan_refusal)
    }

    /// The chat's team; `None` when none has run.
    pub async fn team(&self, thread_id: &str) -> Result<Option<Team>> {
        let state = self.state_of(thread_id).await;
        state
            .team(thread_id, timestamp_ms()?)
            .await
            .map_err(team_refusal)
    }

    /// Passes a task in review whose gate is the user's confirmation. Answers with the
    /// chat that holds the plan and the task as it is now.
    pub async fn confirm_task(&self, thread_id: &str, task_id: &str) -> Result<(String, Task)> {
        let state = self.state_of(thread_id).await;
        let passed = state
            .pass_task(thread_id, task_id, PassedBy::User)
            .await
            .map_err(plan_refusal)?;
        let chat = passed.plan.clone();
        let task = state.present_task(passed).await.map_err(plan_refusal)?;
        // What the task was before is ready now: a running team gives it out.
        self.of(thread_id).await.team_changed(&chat).await;
        Ok((chat, task))
    }

    /// Ends the running team as stopped and gives its tasks at work back to open. Answers
    /// with the team and those tasks.
    ///
    /// The members at work are cancelled with it (`Launcher::team_stop`).
    pub async fn stop_team(&self, thread_id: &str) -> Result<(Team, Vec<Task>)> {
        let state = self.state_of(thread_id).await;
        let now = timestamp_ms()?;
        let chat = state.root_thread(thread_id).await?;
        let launcher = self.of(thread_id).await;
        let returned = launcher.team_stop(&chat).await.map_err(team_refusal)?;
        let mut tasks = Vec::with_capacity(returned.len());
        for task in returned {
            tasks.push(state.present_task(task).await.map_err(plan_refusal)?);
        }
        let team = state
            .team(thread_id, now)
            .await
            .map_err(team_refusal)?
            .ok_or_else(|| anyhow!("no team has run in this chat"))?;
        Ok((team, tasks))
    }

    /// Sets the running team's budget as the user, up to the ceilings of the settings.
    pub async fn set_team_budget(&self, thread_id: &str, budget: TeamBudget) -> Result<Team> {
        let state = self.state_of(thread_id).await;
        let ceilings =
            TeamCeilings::from_limits(&self.config().current().limits()).map_err(team_refusal)?;
        state
            .set_team_budget(thread_id, BudgetBy::User, budget, ceilings)
            .await
            .map_err(team_refusal)?;
        // More members at once, or a budget no longer spent, takes hold now.
        let chat = state.root_thread(thread_id).await?;
        self.of(thread_id).await.team_changed(&chat).await;
        state
            .team(thread_id, timestamp_ms()?)
            .await
            .map_err(team_refusal)?
            .ok_or_else(|| anyhow!("no team has run in this chat"))
    }
}

/// What a client may read of a plan's refusal: the words of a refusal are the user's to
/// act on; a failed store is the core's, so only the log has it.
fn plan_refusal(error: PlanError) -> anyhow::Error {
    match error {
        PlanError::Store(error) => {
            warn!("the plan's store failed: {error:#}");
            anyhow!("the plan could not be read or changed")
        }
        other => anyhow!(other.to_string()),
    }
}

fn team_refusal(error: TeamError) -> anyhow::Error {
    match error {
        TeamError::Store(error) => {
            warn!("the team's store failed: {error:#}");
            anyhow!("the team could not be read or changed")
        }
        TeamError::Plan(error) => anyhow!(
            "the team ended; its work was not returned: {:#}",
            plan_refusal(error)
        ),
        other => anyhow!(other.to_string()),
    }
}
