//! A team: the core's run of the plan of a root thread, one row of `teams` per chat.
//! Starting, reading and ending a run are here; `at_once` changes how many members work
//! at once, `used` counts what the run has used and what the plan holds.
//!
//! Every function finds the chat from the thread that asks, so a thread reaches the team
//! of its own chat and no other. What a run used is never stored: it is counted from
//! `model_usage` and the plan when asked.

mod at_once;
mod error;
mod used;
mod wind;

#[cfg(test)]
mod tests;

use dusk_protocol::thread::{MAX_TEAM_AT_ONCE, Team, TeamEnd, TeamStatus};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

use super::{PlanTask, State, tasks::root_of};
use crate::state::MEMBER_SOURCE;

pub use error::TeamError;
pub use used::{TeamAsk, TeamCheck, TeamCounts, summary as member_summary};
pub use wind::{WindWhy, Winding};

use error::{end_name, parse_end};

type Tx<'a> = Transaction<'a, Sqlite>;

const MINUTE_MS: i64 = 60_000;

/// A team as the store keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRun {
    /// The root thread, which leads it.
    pub thread_id: String,
    pub status: TeamStatus,
    /// Members at work at once.
    pub at_once: u32,
    /// Unix ms; what the run used is counted from here.
    pub started_at: i64,
    pub ended_at: Option<i64>,
    /// Members each work in a tree of their own; false shares the project folder.
    pub worktrees: bool,
    pub base_commit: Option<String>,
    /// What the lead ran it toward; empty before a lead named one.
    pub goal: String,
    /// Set once it winds down; kept after it ended, so its end can say why.
    pub winding: Option<Winding>,
}

// P1 serves `team` and `end_team`; T1 (the team's runtime) is the rest of the callers.
#[allow(dead_code)]
impl State {
    /// Starts a run of the chat's plan at `now` with `at_once` members at work at once
    /// (the setting's). A chat whose team ended runs again in the same row: one team per chat,
    /// its counts start again from the new `started_at`, the plan goes on from where the
    /// last run left it.
    pub async fn start_team(
        &self,
        thread: &str,
        at_once: u32,
        now: i64,
    ) -> Result<TeamRun, TeamError> {
        if !(1..=MAX_TEAM_AT_ONCE).contains(&at_once) {
            return Err(TeamError::PastCeiling {
                ceiling: MAX_TEAM_AT_ONCE,
            });
        }
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
            "INSERT INTO teams (thread_id, status, at_once, started_at)
             VALUES (?1, 'running', ?2, ?3)
             ON CONFLICT (thread_id) DO UPDATE SET status = 'running', ended_why = NULL,
                 ended_at = NULL, at_once = excluded.at_once, started_at = excluded.started_at,
                 worktrees = 0, base_commit = NULL, winding_since = NULL, winding_why = NULL
             WHERE teams.status = 'ended'",
        )
        .bind(&root)
        .bind(i64::from(at_once))
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

    /// Writes the goal the lead runs the chat's team toward. `goal` is checked by the
    /// caller; the column refuses one over `MAX_GOAL`.
    pub async fn set_team_goal(&self, thread: &str, goal: &str) -> Result<(), TeamError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        sqlx::query("UPDATE teams SET goal = ? WHERE thread_id = ?")
            .bind(goal)
            .bind(&root)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// The chats whose team runs, for the dispatcher's tick.
    pub async fn running_teams(&self) -> Result<Vec<String>, TeamError> {
        Ok(
            sqlx::query_scalar("SELECT thread_id FROM teams WHERE status = 'running'")
                .fetch_all(&self.pool)
                .await?,
        )
    }

    /// A thread the team made under `lead` for `role` that never got a task and was
    /// never written to: a claim another task lost, or a stop between making the thread
    /// and the claim. The next claim of that role takes it instead of a new thread.
    pub async fn spare_member(&self, lead: &str, role: &str) -> Result<Option<String>, TeamError> {
        Ok(sqlx::query_scalar(
            "SELECT t.id FROM threads t
             WHERE t.parent_thread_id = ?1 AND t.source = ?2 AND t.agent = ?3
               AND NOT EXISTS (SELECT 1 FROM tasks WHERE owner_thread_id = t.id)
               AND NOT EXISTS (SELECT 1 FROM messages m
                               WHERE m.thread_id = t.id AND m.role != 'system')
               AND NOT EXISTS (SELECT 1 FROM pending_messages p WHERE p.thread_id = t.id)
             ORDER BY t.created_at, t.id LIMIT 1",
        )
        .bind(lead)
        .bind(MEMBER_SOURCE)
        .bind(role)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// The chat's team as stored; `None` when none has run.
    pub async fn team_run(&self, thread: &str) -> Result<Option<TeamRun>, TeamError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        read_run(&mut transaction, &root).await
    }

    /// The chat's team as clients see it: its members at once, what it used by `now` and
    /// its members. `None` when none has run.
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
            at_once: run.at_once,
            used,
            members,
            started_at: Some(run.started_at),
            winding_since: run
                .winding
                .filter(|_| run.status == TeamStatus::Running)
                .map(|winding| winding.since),
            goal: run.goal,
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
        "SELECT thread_id, status, ended_why, at_once, worktrees, base_commit, started_at,
             ended_at, goal, winding_since, winding_why
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
    let worktrees: i64 = row.try_get("worktrees")?;
    let since: Option<i64> = row.try_get("winding_since")?;
    let why: Option<String> = row.try_get("winding_why")?;
    let winding = match (since, why.as_deref()) {
        (Some(since), Some(why)) => Some(Winding {
            since,
            why: WindWhy::parse(why).ok_or_else(|| stored(format!("unknown winding {why}")))?,
        }),
        _ => None,
    };
    Ok(TeamRun {
        thread_id: row.try_get("thread_id")?,
        status,
        at_once: stored_count(row, "at_once")?,
        started_at: row.try_get("started_at")?,
        ended_at: row.try_get("ended_at")?,
        worktrees: worktrees == 1,
        base_commit: row.try_get("base_commit")?,
        goal: row.try_get("goal")?,
        winding,
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
