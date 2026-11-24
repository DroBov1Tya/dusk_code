//! The rows the core adds to a plan by itself, and the moves around them: a review of a
//! task that changed files, the critic at the end, what a member reported, and the lead's
//! `skip` of a task that no longer holds the next wave.
//!
//! A duty row is an ordinary task of the plan: it is given out, leased and released as
//! any other. It differs in three things: it owns no file, its hand-in closes it at once
//! (`close_duty`), and the lead never reviews it.

use dusk_protocol::thread::TaskDuty;
use uuid::Uuid;

use super::{
    State,
    error::PlanError,
    graph::{Tx, load, locate, root_of, settle_blocks},
    plan::{
        MAX_DESCRIPTION, MAX_NOTE, MAX_RESULT, MAX_ROLE, PlanStatus, PlanTask, WaitsOn, capped,
    },
    timestamp_ms,
};

/// A row the core adds: who does it and what it is called on the plan.
#[derive(Debug, Clone, Copy)]
pub struct Duty<'a> {
    pub duty: TaskDuty,
    /// The agent that does it.
    pub role: &'a str,
    pub description: &'a str,
}

impl State {
    /// Writes what the owner reported for its task, whatever the task's status now.
    /// `result` is cut by the caller; the store refuses one over its cap.
    pub async fn report_task(
        &self,
        owner: &str,
        task_id: &str,
        result: &str,
    ) -> Result<(), PlanError> {
        capped("the result", result, MAX_RESULT)?;
        sqlx::query("UPDATE tasks SET result = ? WHERE task_id = ? AND owner_thread_id = ?")
            .bind(result)
            .bind(task_id)
            .bind(owner)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// A task in review that changed files waits for a fresh reviewer: in one
    /// transaction the task waits on a new review row, which depends on what the task
    /// depends on, so it stands in the same wave and is ready at once. Answers the task
    /// and the row.
    pub async fn await_review(
        &self,
        lead: &str,
        task: &str,
        duty: Duty<'_>,
    ) -> Result<(PlanTask, PlanTask), PlanError> {
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, lead).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let reviewed = load(&mut transaction, &root, &task_id).await?;
        if reviewed.status != PlanStatus::Review {
            return Err(PlanError::Illegal {
                task: task_id,
                from: reviewed.status,
                to: PlanStatus::Review,
            });
        }
        let row = insert(
            &mut transaction,
            &root,
            duty,
            &reviewed.depends_on,
            Some(&task_id),
            now,
        )
        .await?;
        sqlx::query(
            "UPDATE tasks SET waits_on = ?, updated_at = ?
             WHERE task_id = ? AND thread_id = ? AND status = 'review'",
        )
        .bind(WaitsOn::Review(row.clone()).stored())
        .bind(now)
        .bind(&task_id)
        .bind(&root)
        .execute(&mut *transaction)
        .await?;
        let reviewed = load(&mut transaction, &root, &task_id).await?;
        let row = load(&mut transaction, &root, &row).await?;
        transaction.commit().await?;
        Ok((reviewed, row))
    }

    /// Adds the critic of the plan's run: a row that depends on nothing and stands one
    /// wave past every other.
    pub async fn add_critic(&self, lead: &str, duty: Duty<'_>) -> Result<PlanTask, PlanError> {
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, lead).await?;
        let row = insert(&mut transaction, &root, duty, &[], None, now).await?;
        let row = load(&mut transaction, &root, &row).await?;
        transaction.commit().await?;
        Ok(row)
    }

    /// running to done for a duty row, with what its member answered: a review or the
    /// critic is closed by its hand-in, never reviewed itself.
    pub async fn close_duty(
        &self,
        owner: &str,
        task: &str,
        result: &str,
    ) -> Result<PlanTask, PlanError> {
        capped("the result", result, MAX_RESULT)?;
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, owner).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let moved = sqlx::query(
            "UPDATE tasks SET status = 'done', done = 1, lease_until = NULL, waits_on = NULL,
                 result = ?, updated_at = ?
             WHERE task_id = ? AND thread_id = ? AND owner_thread_id = ?
               AND status = 'running' AND duty != 'work'",
        )
        .bind(result)
        .bind(now)
        .bind(&task_id)
        .bind(&root)
        .bind(owner)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if moved == 0 {
            let found = load(&mut transaction, &root, &task_id).await?;
            return Err(match found.owner_thread_id.as_deref() == Some(owner) {
                true => PlanError::Illegal {
                    task: task_id,
                    from: found.status,
                    to: PlanStatus::Done,
                },
                false => PlanError::NotOwner { task: task_id },
            });
        }
        let closed = load(&mut transaction, &root, &task_id).await?;
        transaction.commit().await?;
        Ok(closed)
    }

    /// The lead's `skip`: a failed or blocked task is given up, so the next wave does not
    /// wait for it, and with it every task not done that stands after it. A member that
    /// was blocked on it lets its files go. Answers the task first, then the ones behind.
    pub async fn skip_task(
        &self,
        thread: &str,
        task: &str,
        note: &str,
    ) -> Result<Vec<PlanTask>, PlanError> {
        capped("the note", note, MAX_NOTE)?;
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let moved = sqlx::query(
            "UPDATE tasks SET skipped = 1, note = ?, updated_at = ?,
                 owner_thread_id = CASE status WHEN 'blocked' THEN NULL ELSE owner_thread_id END,
                 waits_on = CASE status WHEN 'blocked' THEN NULL ELSE waits_on END
             WHERE task_id = ? AND thread_id = ? AND status IN ('failed', 'blocked')
               AND skipped = 0",
        )
        .bind(note)
        .bind(now)
        .bind(&task_id)
        .bind(&root)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if moved == 0 {
            let found = load(&mut transaction, &root, &task_id).await?;
            return Err(PlanError::Illegal {
                task: task_id,
                from: found.status,
                to: PlanStatus::Done,
            });
        }
        let behind = mark_behind(&mut transaction, &root, &task_id, true, now).await?;
        let mut skipped = vec![load(&mut transaction, &root, &task_id).await?];
        for id in &behind {
            skipped.push(load(&mut transaction, &root, id).await?);
        }
        transaction.commit().await?;
        Ok(skipped)
    }
}

/// Inserts a duty row and answers its id. It owns no file and has no gate.
async fn insert(
    transaction: &mut Tx<'_>,
    root: &str,
    duty: Duty<'_>,
    depends_on: &[String],
    review_of: Option<&str>,
    now: i64,
) -> Result<String, PlanError> {
    capped("the role", duty.role, MAX_ROLE)?;
    let description: String = duty.description.chars().take(MAX_DESCRIPTION).collect();
    let task_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO tasks (task_id, thread_id, description, depends_on, role, duty, review_of,
             created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&task_id)
    .bind(root)
    .bind(description.trim())
    .bind(serde_json::to_string(depends_on)?)
    .bind(duty.role)
    .bind(duty.duty.name())
    .bind(review_of)
    .bind(now)
    .bind(now)
    .execute(&mut **transaction)
    .await?;
    Ok(task_id)
}

/// Sets `skipped` on every task not done and not at work that stands after `task_id`
/// through `depends_on`, the task itself left out. Answers their ids.
pub(super) async fn mark_behind(
    transaction: &mut Tx<'_>,
    root: &str,
    task_id: &str,
    skipped: bool,
    now: i64,
) -> Result<Vec<String>, PlanError> {
    let marked = sqlx::query_scalar(
        "WITH RECURSIVE behind(id) AS (
            SELECT ?1
            UNION
            SELECT t.task_id FROM behind, tasks t, json_each(t.depends_on) d
            WHERE t.thread_id = ?2 AND d.value = behind.id AND t.status != 'done'
         )
         UPDATE tasks SET skipped = ?3, updated_at = ?4
         WHERE thread_id = ?2 AND task_id IN (SELECT id FROM behind) AND task_id != ?1
           AND skipped != ?3 AND status IN ('open', 'blocked') AND owner_thread_id IS NULL
         RETURNING task_id",
    )
    .bind(task_id)
    .bind(root)
    .bind(skipped)
    .bind(now)
    .fetch_all(&mut **transaction)
    .await?;
    settle_blocks(transaction, root, now).await?;
    Ok(marked)
}
