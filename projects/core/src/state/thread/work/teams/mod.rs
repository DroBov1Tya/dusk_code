//! A team: the core's run of the plan of a root thread, one row of `teams` per chat.
//! Starting, reading and ending a run are here; `budget` says who may change the budget
//! and how far, `used` counts what the run has used and what the plan holds.
//!
//! Every function finds the chat from the thread that asks, so a thread reaches the team
//! of its own chat and no other. What a run used is never stored: it is counted from
//! `model_usage` and the plan when asked.

mod budget;
mod error;
mod used;

#[cfg(test)]
mod tests;

use dusk_protocol::thread::{Team, TeamBudget, TeamEnd, TeamStatus};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

use super::{PlanTask, State, tasks::root_of};

pub use budget::{BudgetBy, TeamCeilings};
pub use error::{BudgetPart, TeamError};
pub use used::{Spent, TeamAsk, TeamCheck, TeamCounts};

use error::{end_name, parse_end};

type Tx<'a> = Transaction<'a, Sqlite>;

const MINUTE_MS: i64 = 60_000;

/// A team as the store keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRun {
    /// The root thread, which leads it.
    pub thread_id: String,
    pub status: TeamStatus,
    pub budget: TeamBudget,
    /// Unix ms; what the run used is counted from here.
    pub started_at: i64,
    /// Unix ms after which nothing is given out: `started_at` and the budget's minutes.
    pub deadline: i64,
    pub ended_at: Option<i64>,
    /// Members each work in a tree of their own; false shares the project folder.
    pub worktrees: bool,
    pub base_commit: Option<String>,
}

// P1 serves `team` and `end_team`; T1 (the team's runtime) is the rest of the callers.
#[allow(dead_code)]
impl State {
    /// Starts a run of the chat's plan at `now` with `budget`, which must be within
    /// `ceilings`. A chat whose team ended runs again in the same row: one team per chat,
    /// its counts start again from the new `started_at`, the plan goes on from where the
    /// last run left it.
    pub async fn start_team(
        &self,
        thread: &str,
        budget: TeamBudget,
        ceilings: TeamCeilings,
        now: i64,
    ) -> Result<TeamRun, TeamError> {
        ceilings.admit(budget)?;
        let deadline = deadline(now, budget)?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        if let Some(run) = read_run(&mut transaction, &root).await?
            && run.status == TeamStatus::Running
        {
            return Err(TeamError::Running);
        }
        let at_work = used::at_work(&mut transaction, &root).await?;
        if at_work > 0 {
            return Err(TeamError::WorkLeft(at_work));
        }
        let started = sqlx::query(
            "INSERT INTO teams (thread_id, status, at_once, max_steps, max_tokens, deadline,
                 started_at)
             VALUES (?1, 'running', ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (thread_id) DO UPDATE SET status = 'running', ended_why = NULL,
                 ended_at = NULL, at_once = excluded.at_once, max_steps = excluded.max_steps,
                 max_tokens = excluded.max_tokens, deadline = excluded.deadline,
                 started_at = excluded.started_at, worktrees = 0, base_commit = NULL
             WHERE teams.status = 'ended'",
        )
        .bind(&root)
        .bind(i64::from(budget.at_once()))
        .bind(i64::from(budget.steps()))
        .bind(sql_count(budget.tokens())?)
        .bind(deadline)
        .bind(now)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if started == 0 {
            return Err(TeamError::Running);
        }
        let run = read_run(&mut transaction, &root)
            .await?
            .ok_or(TeamError::NoTeam)?;
        transaction.commit().await?;
        Ok(run)
    }

    /// The chat's team as stored; `None` when none has run.
    pub async fn team_run(&self, thread: &str) -> Result<Option<TeamRun>, TeamError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        read_run(&mut transaction, &root).await
    }

    /// The chat's team as clients see it: its budget, what it used by `now` and its
    /// members. `None` when none has run.
    pub async fn team(&self, thread: &str, now: i64) -> Result<Option<Team>, TeamError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let Some(run) = read_run(&mut transaction, &root).await? else {
            return Ok(None);
        };
        let used = used::used(&mut transaction, &run, now).await?;
        let members = used::members(&mut transaction, &root).await?;
        Ok(Some(Team {
            thread_id: run.thread_id,
            status: run.status,
            budget: run.budget,
            used,
            members,
        }))
    }

    /// Ends the running team with `why` at `now`, then gives its tasks at work back to
    /// open (`return_work`, its own transaction), and answers with both.
    ///
    /// The end commits first. A crash between the two leaves an ended team with tasks
    /// claimed or running: nothing gives them out again, a new run is refused with
    /// `WorkLeft`, and recovery returns them (`team_counts().at_work > 0` on an ended
    /// team: call `return_work`). The other order could lose the end, and a stopped team
    /// would run again after a restart. A failed return is `TeamError::Plan`, the end
    /// standing. The members' threads are the caller's to cancel; it reads them before,
    /// since a returned task has no owner.
    pub async fn end_team(
        &self,
        thread: &str,
        why: TeamEnd,
        now: i64,
    ) -> Result<(TeamRun, Vec<PlanTask>), TeamError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let ended = sqlx::query(
            "UPDATE teams SET status = 'ended', ended_why = ?, ended_at = ?
             WHERE thread_id = ? AND status = 'running'",
        )
        .bind(end_name(why))
        .bind(now)
        .bind(&root)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        let run = read_run(&mut transaction, &root)
            .await?
            .ok_or(TeamError::NoTeam)?;
        if ended == 0 {
            return Err(match run.status {
                TeamStatus::Ended(before) => TeamError::Ended(before),
                TeamStatus::Running => TeamError::Running,
            });
        }
        transaction.commit().await?;
        let returned = self.return_work(&root).await.map_err(TeamError::Plan)?;
        Ok((run, returned))
    }
}

/// The team row of a root thread.
async fn read_run(transaction: &mut Tx<'_>, root: &str) -> Result<Option<TeamRun>, TeamError> {
    sqlx::query(
        "SELECT thread_id, status, ended_why, at_once, max_steps, max_tokens, deadline,
             worktrees, base_commit, started_at, ended_at
         FROM teams WHERE thread_id = ?",
    )
    .bind(root)
    .fetch_optional(&mut **transaction)
    .await?
    .as_ref()
    .map(decode)
    .transpose()
}

fn decode(row: &SqliteRow) -> Result<TeamRun, TeamError> {
    let status: String = row.try_get("status")?;
    let ended_why: Option<String> = row.try_get("ended_why")?;
    let status = match (status.as_str(), ended_why.as_deref()) {
        ("running", None) => TeamStatus::Running,
        ("ended", Some(why)) => TeamStatus::Ended(
            parse_end(why).ok_or_else(|| stored(format!("unknown team end {why}")))?,
        ),
        _ => return Err(stored(format!("unknown team status {status}"))),
    };
    let started_at: i64 = row.try_get("started_at")?;
    let deadline: i64 = row.try_get("deadline")?;
    let minutes = deadline
        .checked_sub(started_at)
        .map(|span| span / MINUTE_MS)
        .and_then(|minutes| u32::try_from(minutes).ok())
        .ok_or_else(|| stored("the team's deadline is out of range".to_owned()))?;
    let budget = TeamBudget::new(
        stored_count(row, "at_once")?,
        stored_count(row, "max_steps")?,
        stored_count(row, "max_tokens")?,
        minutes,
    )
    .map_err(TeamError::Store)?;
    let worktrees: i64 = row.try_get("worktrees")?;
    Ok(TeamRun {
        thread_id: row.try_get("thread_id")?,
        status,
        budget,
        started_at,
        deadline,
        ended_at: row.try_get("ended_at")?,
        worktrees: worktrees == 1,
        base_commit: row.try_get("base_commit")?,
    })
}

/// A count column, refused when it does not fit the type it is read into.
fn stored_count<Count: TryFrom<i64>>(row: &SqliteRow, column: &str) -> Result<Count, TeamError> {
    let value: i64 = row.try_get(column)?;
    Count::try_from(value).map_err(|_| stored(format!("{column} {value} is out of range")))
}

fn stored(why: String) -> TeamError {
    TeamError::Store(anyhow::anyhow!(why))
}

fn sql_count(count: u64) -> Result<i64, TeamError> {
    i64::try_from(count).map_err(|_| stored(format!("{count} is past what SQLite holds")))
}

/// `now` and the budget's minutes, as a Unix ms.
fn deadline(now: i64, budget: TeamBudget) -> Result<i64, TeamError> {
    i64::from(budget.minutes())
        .checked_mul(MINUTE_MS)
        .and_then(|span| now.checked_add(span))
        .ok_or_else(|| stored(format!("a deadline from {now} is out of range")))
}
