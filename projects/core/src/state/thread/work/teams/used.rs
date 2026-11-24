//! What a run has used, counted when asked and only shown, the check before each claim
//! and step, the members, and the plan's counts the stall rule reads.

use dusk_protocol::thread::{
    MAX_TEAM_MEMBERS, TaskStatus, TeamEnd, TeamMember, TeamStatus, TeamUsed,
};
use sqlx::Row;

use super::{MINUTE_MS, State, TeamRun, Tx, error::TeamError, read_run, root_of, stored};
use crate::state::{PlanStatus, thread::threads::family};

/// What the dispatcher is about to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamAsk {
    /// Give out another task.
    Claim,
    /// Let a member at work take another model step.
    Step,
}

/// What `team_check` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamCheck {
    Go,
    /// As many members are at work as may be at once: wait for one, the team goes on.
    Full,
    /// The team is not running.
    Ended(TeamEnd),
}

/// The plan's tasks by where they stand, for the stall rule and the lead's line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TeamCounts {
    pub total: u32,
    /// Claimed or running.
    pub at_work: u32,
    /// Open, every dependency done, in the current wave.
    pub ready: u32,
    pub review: u32,
    /// In review for the lead: without a gate, or held for it.
    pub for_lead: u32,
    /// In review for the user's confirmation.
    pub for_user: u32,
    pub blocked: u32,
    pub failed: u32,
    pub done: u32,
    /// Given up by the lead, whatever their status: they hold no wave.
    pub skipped: u32,
}

// T1 (the dispatcher, the stall rule) and R1 (recovery) are the callers; nothing in the
// tree is yet.
#[allow(dead_code)]
impl State {
    /// What the chat's team has used by `now`.
    pub async fn team_used(&self, thread: &str, now: i64) -> Result<TeamUsed, TeamError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let run = read_run(&mut transaction, &root)
            .await?
            .ok_or(TeamError::NoTeam)?;
        used(&mut transaction, &run, now).await
    }

    /// Whether the chat's team may give out another task, or let a member take another
    /// step. Members at once count for a claim only; nothing else bounds a running team.
    pub async fn team_check(&self, thread: &str, ask: TeamAsk) -> Result<TeamCheck, TeamError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let run = read_run(&mut transaction, &root)
            .await?
            .ok_or(TeamError::NoTeam)?;
        if let TeamStatus::Ended(why) = run.status {
            return Ok(TeamCheck::Ended(why));
        }
        if ask == TeamAsk::Claim && at_work(&mut transaction, &root).await? >= run.at_once {
            return Ok(TeamCheck::Full);
        }
        Ok(TeamCheck::Go)
    }

    /// The threads that hold a task of the chat's plan, newest first, each with the task
    /// it works on, or else the one it touched last.
    pub async fn team_members(&self, thread: &str) -> Result<Vec<TeamMember>, TeamError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        members(&mut transaction, &root).await
    }

    /// The chat's plan by where its tasks stand. A plan with no team has counts too.
    pub async fn team_counts(&self, thread: &str) -> Result<TeamCounts, TeamError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        // A dependency whose row is gone counts as not done, as in `ready_tasks`.
        let row = sqlx::query(
            "SELECT COUNT(*) AS total,
                 COALESCE(SUM(status IN ('claimed', 'running')), 0) AS at_work,
                 COALESCE(SUM(status = 'review'), 0) AS review,
                 COALESCE(SUM(status = 'review' AND (waits_on = 'lead'
                     OR (gate IS NULL AND waits_on IS NULL))), 0) AS for_lead,
                 COALESCE(SUM(status = 'review' AND waits_on IS NULL
                     AND json_extract(gate, '$.kind') = 'user_confirmation'), 0) AS for_user,
                 COALESCE(SUM(status = 'blocked' AND skipped = 0), 0) AS blocked,
                 COALESCE(SUM(status = 'failed' AND skipped = 0), 0) AS failed,
                 COALESCE(SUM(status = 'done'), 0) AS done,
                 COALESCE(SUM(skipped = 1 AND status != 'done'), 0) AS skipped
             FROM tasks t WHERE t.thread_id = ?",
        )
        .bind(&root)
        .fetch_one(&mut *transaction)
        .await?;
        let count = |column: &str| -> Result<u32, TeamError> {
            let value: i64 = row.try_get(column)?;
            u32::try_from(value).map_err(|_| stored(format!("{column} {value} is out of range")))
        };
        drop(transaction);
        // The barrier decides what is ready: the same rule the dispatcher gives out by.
        let ready = self.ready_tasks(thread).await?.len();
        Ok(TeamCounts {
            total: count("total")?,
            at_work: count("at_work")?,
            ready: u32::try_from(ready).unwrap_or(u32::MAX),
            review: count("review")?,
            for_lead: count("for_lead")?,
            for_user: count("for_user")?,
            blocked: count("blocked")?,
            failed: count("failed")?,
            done: count("done")?,
            skipped: count("skipped")?,
        })
    }
}

/// The run's use: steps are the answering calls of the root's tree, tokens the prompt
/// and completion of every call of it, both from `started_at` to the end of the run;
/// members at once are the tasks claimed or running; minutes run to `now` or the end.
pub(super) async fn used(
    transaction: &mut Tx<'_>,
    run: &TeamRun,
    now: i64,
) -> Result<TeamUsed, TeamError> {
    let until = run.ended_at.unwrap_or(i64::MAX);
    let (steps, tokens): (i64, i64) = sqlx::query_as(family!(
        "SELECT
             (SELECT COUNT(*) FROM model_usage u
              WHERE u.thread_id IN (SELECT id FROM family) AND u.purpose = 'answer'
                AND u.created_at BETWEEN ?2 AND ?3),
             (SELECT COALESCE(SUM(u.prompt_tokens + u.completion_tokens), 0) FROM model_usage u
              WHERE u.thread_id IN (SELECT id FROM family) AND u.created_at BETWEEN ?2 AND ?3)"
    ))
    .bind(&run.thread_id)
    .bind(run.started_at)
    .bind(until)
    .fetch_one(&mut **transaction)
    .await?;
    let elapsed = run
        .ended_at
        .unwrap_or(now)
        .saturating_sub(run.started_at)
        .max(0);
    Ok(TeamUsed {
        at_once: at_work(transaction, &run.thread_id).await?,
        steps: u32::try_from(steps).map_err(|_| stored(format!("{steps} steps")))?,
        tokens: u64::try_from(tokens).map_err(|_| stored(format!("{tokens} tokens")))?,
        // A clock that went back counts as no time; past u32 minutes shows as the most.
        minutes: u32::try_from(elapsed / MINUTE_MS).unwrap_or(u32::MAX),
    })
}

/// Tasks of the plan claimed or running.
pub(super) async fn at_work(transaction: &mut Tx<'_>, root: &str) -> Result<u32, TeamError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE thread_id = ? AND status IN ('claimed', 'running')",
    )
    .bind(root)
    .fetch_one(&mut **transaction)
    .await?;
    u32::try_from(count).map_err(|_| stored(format!("{count} tasks at work")))
}

/// Each thread that owns a task of the plan once, with the task it holds now (not done
/// or failed; at most one, as the claim keeps it), else the one it touched last.
pub(super) async fn members(
    transaction: &mut Tx<'_>,
    root: &str,
) -> Result<Vec<TeamMember>, TeamError> {
    let rows = sqlx::query(
        "SELECT th.id, COALESCE(th.name, th.id) AS name, t.task_id, t.role, t.model, t.status
         FROM (SELECT task_id, owner_thread_id, role, model, status,
                   ROW_NUMBER() OVER (PARTITION BY owner_thread_id
                       ORDER BY status NOT IN ('done', 'failed') DESC, updated_at DESC,
                                task_id DESC) AS rank
               FROM tasks WHERE thread_id = ?1 AND owner_thread_id IS NOT NULL) t
         JOIN threads th ON th.id = t.owner_thread_id
         WHERE t.rank = 1
         ORDER BY th.created_at DESC, th.id DESC
         LIMIT ?2",
    )
    .bind(root)
    .bind(i64::try_from(MAX_TEAM_MEMBERS).unwrap_or(i64::MAX))
    .fetch_all(&mut **transaction)
    .await?;
    rows.iter()
        .map(|row| {
            let status: String = row.try_get("status")?;
            let status = PlanStatus::parse(&status)
                .ok_or_else(|| stored(format!("unknown task status {status}")))?;
            Ok(TeamMember {
                thread_id: row.try_get("id")?,
                name: row.try_get("name")?,
                role: row.try_get("role")?,
                model: row.try_get("model")?,
                task_id: row.try_get("task_id")?,
                status: task_status(status),
            })
        })
        .collect()
}

fn task_status(status: PlanStatus) -> TaskStatus {
    match status {
        PlanStatus::Open => TaskStatus::Open,
        PlanStatus::Claimed => TaskStatus::Claimed,
        PlanStatus::Running => TaskStatus::Running,
        PlanStatus::Blocked => TaskStatus::Blocked,
        PlanStatus::Review => TaskStatus::Review,
        PlanStatus::Done => TaskStatus::Done,
        PlanStatus::Failed => TaskStatus::Failed,
    }
}
