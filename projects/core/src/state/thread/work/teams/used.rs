//! What a run has used of its budget, counted when asked (section 5 of
//! `docs/plan-and-teams.md`), the check before each claim and step, the members, and
//! the plan's counts the stall rule reads.

use dusk_protocol::thread::{
    MAX_TEAM_MEMBERS, TaskStatus, TeamEnd, TeamMember, TeamStatus, TeamUsed,
};
use sqlx::Row;

use super::{MINUTE_MS, State, TeamRun, Tx, error::TeamError, read_run, root_of, stored};
use crate::state::PlanStatus;

/// What the dispatcher is about to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamAsk {
    /// Give out another task.
    Claim,
    /// Let a member at work take another model step.
    Step,
}

/// The budget a run has spent; the team ends with `end()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spent {
    Steps,
    Tokens,
    Time,
}

/// What `team_check` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamCheck {
    Go,
    /// As many members are at work as the budget allows: wait for one, the team goes on.
    Full,
    /// A budget is spent: the team ends.
    Spent(Spent),
    /// The team is not running.
    Ended(TeamEnd),
}

/// The plan's tasks by where they stand, for the stall rule and the lead's line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TeamCounts {
    pub total: u32,
    /// Claimed or running.
    pub at_work: u32,
    /// Open, every dependency done.
    pub ready: u32,
    pub review: u32,
    /// In review for the lead: without a gate, or held for it.
    pub for_lead: u32,
    /// In review for the user's confirmation.
    pub for_user: u32,
    pub blocked: u32,
    pub failed: u32,
    pub done: u32,
}

impl Spent {
    pub fn end(self) -> TeamEnd {
        match self {
            Self::Steps => TeamEnd::Steps,
            Self::Tokens => TeamEnd::Tokens,
            Self::Time => TeamEnd::Time,
        }
    }
}

/// The root thread and every thread under it, as `tree(id)`, for a query that binds the
/// root as `?1`. UNION and the bound of 32 levels end the walk on parent links that
/// loop, which only a damaged database has; agents nest far less deep.
macro_rules! tree {
    ($rest:literal) => {
        concat!(
            "WITH RECURSIVE tree(id, depth) AS (
                SELECT ?1, 0
                UNION
                SELECT threads.id, tree.depth + 1
                FROM threads JOIN tree ON threads.parent_thread_id = tree.id
                WHERE tree.depth < 32
             ) ",
            $rest
        )
    };
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
    /// step, at `now`; if not, which budget is spent. Time first, then steps, then tokens;
    /// members at once count for a claim only.
    pub async fn team_check(
        &self,
        thread: &str,
        ask: TeamAsk,
        now: i64,
    ) -> Result<TeamCheck, TeamError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let run = read_run(&mut transaction, &root)
            .await?
            .ok_or(TeamError::NoTeam)?;
        if let TeamStatus::Ended(why) = run.status {
            return Ok(TeamCheck::Ended(why));
        }
        if now >= run.deadline {
            return Ok(TeamCheck::Spent(Spent::Time));
        }
        let used = used(&mut transaction, &run, now).await?;
        Ok(if used.steps >= run.budget.steps() {
            TeamCheck::Spent(Spent::Steps)
        } else if used.tokens >= run.budget.tokens() {
            TeamCheck::Spent(Spent::Tokens)
        } else if ask == TeamAsk::Claim && used.at_once >= run.budget.at_once() {
            TeamCheck::Full
        } else {
            TeamCheck::Go
        })
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
                 COALESCE(SUM(status = 'open' AND NOT EXISTS (
                     SELECT 1 FROM json_each(t.depends_on) d
                     LEFT JOIN tasks dep ON dep.task_id = d.value AND dep.thread_id = t.thread_id
                     WHERE dep.status IS NOT 'done')), 0) AS ready,
                 COALESCE(SUM(status = 'review'), 0) AS review,
                 COALESCE(SUM(status = 'review' AND (gate IS NULL OR waits_on = 'lead')), 0)
                     AS for_lead,
                 COALESCE(SUM(status = 'review' AND waits_on IS NOT 'lead'
                     AND json_extract(gate, '$.kind') = 'user_confirmation'), 0) AS for_user,
                 COALESCE(SUM(status = 'blocked'), 0) AS blocked,
                 COALESCE(SUM(status = 'failed'), 0) AS failed,
                 COALESCE(SUM(status = 'done'), 0) AS done
             FROM tasks t WHERE t.thread_id = ?",
        )
        .bind(&root)
        .fetch_one(&mut *transaction)
        .await?;
        let count = |column: &str| -> Result<u32, TeamError> {
            let value: i64 = row.try_get(column)?;
            u32::try_from(value).map_err(|_| stored(format!("{column} {value} is out of range")))
        };
        Ok(TeamCounts {
            total: count("total")?,
            at_work: count("at_work")?,
            ready: count("ready")?,
            review: count("review")?,
            for_lead: count("for_lead")?,
            for_user: count("for_user")?,
            blocked: count("blocked")?,
            failed: count("failed")?,
            done: count("done")?,
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
    let (steps, tokens): (i64, i64) = sqlx::query_as(tree!(
        "SELECT
             (SELECT COUNT(*) FROM model_usage u
              WHERE u.thread_id IN (SELECT id FROM tree) AND u.purpose = 'answer'
                AND u.created_at BETWEEN ?2 AND ?3),
             (SELECT COALESCE(SUM(u.prompt_tokens + u.completion_tokens), 0) FROM model_usage u
              WHERE u.thread_id IN (SELECT id FROM tree) AND u.created_at BETWEEN ?2 AND ?3)"
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
        // A clock that went back counts as no time; past u32 minutes is past any budget.
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
