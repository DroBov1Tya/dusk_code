//! Who may change a team's budget, and how far. A part may always go down; it goes up
//! only when the user asks, and never past the user's ceiling. The lead's `run` only
//! lowers; a member has no way to name a budget at all.

use dusk_protocol::thread::{TeamBudget, TeamStatus};

use super::{
    State, TeamRun, deadline,
    error::{BudgetPart, TeamError},
    read_run, root_of, sql_count,
};
use crate::config::Limits;

/// The user's ceilings for a team's budget: `limits.team_*`, members at once cut to
/// `max_agents_at_work`. A type of its own, so a ceiling is never passed for a budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamCeilings(TeamBudget);

/// Who asks to change a running team's budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetBy {
    /// The lead's `run`: it may lower a part, never raise one.
    Lead,
    /// The user's `SetTeamBudget`: it may raise a part up to its ceiling.
    User,
}

// T1 and P1 are the callers; nothing in the tree is yet.
#[allow(dead_code)]
impl TeamCeilings {
    pub fn new(ceilings: TeamBudget) -> Self {
        Self(ceilings)
    }

    pub fn from_limits(limits: &Limits) -> Result<Self, TeamError> {
        let at_once = u32::try_from(limits.team_members_at_once()).unwrap_or(u32::MAX);
        TeamBudget::new(
            at_once,
            limits.team_max_steps,
            limits.team_max_tokens,
            limits.team_max_minutes,
        )
        .map(Self)
        .map_err(TeamError::Store)
    }

    /// The budget of a run whose lead names none: the ceilings themselves, as the
    /// defaults of the settings are.
    pub fn budget(self) -> TeamBudget {
        self.0
    }

    /// Refuses a budget with a part above its ceiling.
    pub(super) fn admit(self, asked: TeamBudget) -> Result<(), TeamError> {
        for ((part, want), (_, ceiling)) in parts(asked).into_iter().zip(parts(self.0)) {
            if want > ceiling {
                return Err(TeamError::PastCeiling { part, ceiling });
            }
        }
        Ok(())
    }
}

/// Every part of a budget, in one unit each can be compared in.
fn parts(budget: TeamBudget) -> [(BudgetPart, u64); 4] {
    [
        (BudgetPart::AtOnce, u64::from(budget.at_once())),
        (BudgetPart::Steps, u64::from(budget.steps())),
        (BudgetPart::Tokens, budget.tokens()),
        (BudgetPart::Minutes, u64::from(budget.minutes())),
    ]
}

/// Whether `by` may change `current` to `asked`, part by part.
pub(super) fn may_change(
    by: BudgetBy,
    current: TeamBudget,
    asked: TeamBudget,
    ceilings: TeamCeilings,
) -> Result<(), TeamError> {
    let changes = parts(current).into_iter().zip(parts(asked));
    for (((part, now), (_, want)), (_, ceiling)) in changes.zip(parts(ceilings.0)) {
        if want <= now {
            continue;
        }
        match by {
            BudgetBy::Lead => return Err(TeamError::LeadRaises(part)),
            BudgetBy::User if want > ceiling => {
                return Err(TeamError::PastCeiling { part, ceiling });
            }
            BudgetBy::User => {}
        }
    }
    Ok(())
}

// T1 and P1 are the callers; nothing in the tree is yet.
#[allow(dead_code)]
impl State {
    /// Sets the running team's budget, as `by` may (`may_change`). Minutes count from the
    /// run's start, so the deadline moves with them.
    pub async fn set_team_budget(
        &self,
        thread: &str,
        by: BudgetBy,
        budget: TeamBudget,
        ceilings: TeamCeilings,
    ) -> Result<TeamRun, TeamError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let run = read_run(&mut transaction, &root)
            .await?
            .ok_or(TeamError::NoTeam)?;
        if let TeamStatus::Ended(why) = run.status {
            return Err(TeamError::Ended(why));
        }
        may_change(by, run.budget, budget, ceilings)?;
        let changed = sqlx::query(
            "UPDATE teams SET at_once = ?, max_steps = ?, max_tokens = ?, deadline = ?
             WHERE thread_id = ? AND status = 'running'",
        )
        .bind(i64::from(budget.at_once()))
        .bind(i64::from(budget.steps()))
        .bind(sql_count(budget.tokens())?)
        .bind(deadline(run.started_at, budget)?)
        .bind(&root)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if changed != 1 {
            return Err(TeamError::Store(anyhow::anyhow!(
                "the team changed while its budget was set"
            )));
        }
        let run = read_run(&mut transaction, &root)
            .await?
            .ok_or(TeamError::NoTeam)?;
        transaction.commit().await?;
        Ok(run)
    }
}
