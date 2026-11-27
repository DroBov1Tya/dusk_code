//! Every move of a task from one status to another (the state machine of a
//! task), one function and one transaction each. A move the table
//! does not have is refused with `PlanError::Illegal` and changes nothing.
//!
//! The store cannot know who calls. What it can check it does: the plan is found from
//! the thread that asks, a member's moves name the owner thread, and a review is passed
//! only by the one its gate names.

use dusk_protocol::thread::GoalCheck;

use super::{
    State,
    duties::mark_behind,
    error::{PlanError, Unclaimable},
    files::TaskFiles,
    graph::{Tx, load, locate, root_of, settle_blocks},
    plan::{
        BlockedOn, LEASE_MS, MAX_ATTEMPTS, MAX_MODEL, MAX_NOTE, MAX_REWORKS, PlanStatus, PlanTask,
        WaitsOn, capped,
    },
    timestamp_ms,
};

/// What the core fixes when it gives a task out.
#[derive(Debug, Clone, Copy)]
pub struct Claim<'a> {
    /// The member's thread, under the plan's chat.
    pub owner: &'a str,
    /// How many tasks the team has at work at most.
    pub at_once: u32,
    /// The model the member answers on, as resolved now.
    pub model: Option<&'a str>,
    /// Unix ms; the lease runs from it.
    pub now: i64,
}

/// Who says a task in review is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassedBy {
    /// The core: the gate's command exited 0.
    Gate,
    /// The lead's `review`: a task without a gate, or one the core held for it.
    Lead,
    /// The user's `ConfirmTask`, for a `user_confirmation` gate.
    User,
    /// The core: a fresh reviewer approved it.
    Review,
}

/// Two entries of `files` overlap. The same rule as `files::overlaps`.
macro_rules! overlap {
    () => {
        "(rtrim(mine.value, '/') = rtrim(theirs.value, '/')
          OR (substr(mine.value, -1) = '/'
              AND substr(theirs.value, 1, length(mine.value)) = mine.value)
          OR (substr(theirs.value, -1) = '/'
              AND substr(mine.value, 1, length(theirs.value)) = theirs.value))"
    };
}

/// A task holds its files while it is at work or in review, and while its member is
/// blocked half way through it.
macro_rules! holds_files {
    () => {
        "(o.status IN ('claimed', 'running', 'review')
          OR (o.status = 'blocked' AND o.owner_thread_id IS NOT NULL))"
    };
}

// T1 and L1 are the callers; nothing in the tree is yet.
#[allow(dead_code)]
impl State {
    /// open to claimed: gives the task to `claim.owner`, if it is open, everything it
    /// depends on is done, no task holds an overlapping file, fewer than `at_once` are at
    /// work and the owner has no unfinished task. One statement, so of two claims of one
    /// task one wins.
    pub async fn claim_task(
        &self,
        thread: &str,
        task: &str,
        claim: Claim<'_>,
    ) -> Result<PlanTask, PlanError> {
        capped("the model", claim.model.unwrap_or_default(), MAX_MODEL)?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        if root_of(&mut transaction, claim.owner).await? != root {
            return Err(PlanError::NotClaimable {
                task: task_id,
                why: Unclaimable::ForeignOwner,
            });
        }
        let claimed = sqlx::query(concat!(
            "UPDATE tasks AS t SET status = 'claimed', owner_thread_id = ?3, lease_until = ?4,
                 attempts = attempts + 1, model = ?5, waits_on = NULL, updated_at = ?6,
                 resumes = resumes + (t.last_owner_thread_id IS ?3)
             WHERE t.task_id = ?1 AND t.thread_id = ?2 AND t.status = 'open'
               AND t.skipped = 0
               AND NOT EXISTS (
                   SELECT 1 FROM json_each(t.depends_on) d
                   LEFT JOIN tasks dep ON dep.task_id = d.value AND dep.thread_id = t.thread_id
                   WHERE dep.status IS NOT 'done')
               AND NOT EXISTS (
                   SELECT 1 FROM tasks o, json_each(o.files) theirs, json_each(t.files) mine
                   WHERE o.thread_id = t.thread_id AND o.task_id != t.task_id
                     AND ",
            holds_files!(),
            " AND ",
            overlap!(),
            ")
               AND (SELECT COUNT(*) FROM tasks w WHERE w.thread_id = t.thread_id
                    AND w.status IN ('claimed', 'running')) < ?7
               AND NOT EXISTS (
                   SELECT 1 FROM tasks b WHERE b.owner_thread_id = ?3
                     AND b.status NOT IN ('done', 'failed'))"
        ))
        .bind(&task_id)
        .bind(&root)
        .bind(claim.owner)
        .bind(claim.now.saturating_add(LEASE_MS))
        .bind(claim.model)
        .bind(claim.now)
        .bind(i64::from(claim.at_once))
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if claimed == 0 {
            return Err(why_not_claimed(&mut transaction, &root, &task_id, claim).await?);
        }
        finish(transaction, &root, &task_id).await
    }

    /// claimed to running: the member's turn started. The turn is marked with the task,
    /// so its changes and tokens are found by a join.
    pub async fn start_task(
        &self,
        owner: &str,
        task: &str,
        turn_id: &str,
    ) -> Result<PlanTask, PlanError> {
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, owner).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let moved = sqlx::query(
            "UPDATE tasks SET status = 'running', updated_at = ?
             WHERE task_id = ? AND thread_id = ? AND owner_thread_id = ? AND status = 'claimed'",
        )
        .bind(now)
        .bind(&task_id)
        .bind(&root)
        .bind(owner)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if moved == 0 {
            return Err(refusal(
                &mut transaction,
                &root,
                &task_id,
                Some(owner),
                PlanStatus::Running,
            )
            .await);
        }
        let marked =
            sqlx::query("UPDATE turns SET task_id = ? WHERE turn_id = ? AND thread_id = ?")
                .bind(&task_id)
                .bind(turn_id)
                .bind(owner)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
        if marked == 0 {
            return Err(PlanError::NoSuchTurn(turn_id.to_owned()));
        }
        finish(transaction, &root, &task_id).await
    }

    /// Marks a later turn of the owner with the task it holds running: the turn that
    /// redoes it after a review. Without the mark that turn's changes and tokens would
    /// not be counted as the task's. A turn that is marked already stays as it is.
    pub async fn mark_task_turn(
        &self,
        owner: &str,
        task_id: &str,
        turn_id: &str,
    ) -> Result<(), PlanError> {
        sqlx::query(
            "UPDATE turns SET task_id = ?1 WHERE turn_id = ?2 AND thread_id = ?3
                 AND task_id IS NULL
                 AND EXISTS (SELECT 1 FROM tasks WHERE task_id = ?1
                     AND owner_thread_id = ?3 AND status = 'running')",
        )
        .bind(task_id)
        .bind(turn_id)
        .bind(owner)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The files of the task `owner` holds now; `None` when it holds none. What a
    /// member may write about outside its own work (the project graph) is cut to these.
    pub async fn held_task_files(&self, owner: &str) -> Result<Option<TaskFiles>, PlanError> {
        let files: Option<String> = sqlx::query_scalar(
            "SELECT files FROM tasks
             WHERE owner_thread_id = ? AND status IN ('claimed', 'running') LIMIT 1",
        )
        .bind(owner)
        .fetch_optional(&self.pool)
        .await?;
        let Some(files) = files else {
            return Ok(None);
        };
        let entries: Vec<String> = serde_json::from_str(&files)?;
        Ok(Some(TaskFiles::new(entries)?))
    }

    /// Moves the lease on, for the owner's runner alone: the statement names the thread,
    /// so no other thread and no tool can hold a task it was not given. `task_id` is whole.
    pub async fn renew_lease(&self, owner: &str, task_id: &str, now: i64) -> Result<(), PlanError> {
        let mut transaction = self.pool.begin().await?;
        let renewed = sqlx::query(
            "UPDATE tasks SET lease_until = ?
             WHERE task_id = ? AND owner_thread_id = ? AND status IN ('claimed', 'running')",
        )
        .bind(now.saturating_add(LEASE_MS))
        .bind(task_id)
        .bind(owner)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if renewed == 0 {
            let root = root_of(&mut transaction, owner).await?;
            let task_id = locate(&mut transaction, &root, task_id).await?;
            return Err(refusal(
                &mut transaction,
                &root,
                &task_id,
                Some(owner),
                PlanStatus::Running,
            )
            .await);
        }
        transaction.commit().await?;
        Ok(())
    }

    /// claimed or running back to open, the owner cleared and kept as the last owner: the
    /// member's turn failed or was stopped. At 3 attempts the task fails instead and keeps
    /// its owner.
    pub async fn release_task(
        &self,
        thread: &str,
        task: &str,
        note: &str,
    ) -> Result<PlanTask, PlanError> {
        capped("the note", note, MAX_NOTE)?;
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let released = release(
            &mut transaction,
            &root,
            Some(&task_id),
            None,
            note,
            now,
            true,
        )
        .await?;
        if released.is_empty() {
            return Err(refusal(&mut transaction, &root, &task_id, None, PlanStatus::Open).await);
        }
        settle_blocks(&mut transaction, &root, now).await?;
        finish(transaction, &root, &task_id).await
    }

    /// claimed or running back to open, the owner cleared and kept as the last owner,
    /// without counting the attempt: what took the task away was not its member's doing (a restart, the
    /// provider, a team that winds down). `owner` must hold it, which keeps a second
    /// return or a claim that came between from being undone.
    pub async fn return_task(
        &self,
        owner: &str,
        task: &str,
        note: &str,
    ) -> Result<PlanTask, PlanError> {
        capped("the note", note, MAX_NOTE)?;
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, owner).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        // The claim counted the attempt; this takes it back.
        let returned = sqlx::query(
            "UPDATE tasks SET status = 'open', owner_thread_id = NULL, lease_until = NULL,
                 last_owner_thread_id = owner_thread_id,
                 attempts = MAX(attempts - 1, 0), note = ?, updated_at = ?
             WHERE task_id = ? AND thread_id = ? AND owner_thread_id = ?
               AND status IN ('claimed', 'running')",
        )
        .bind(note)
        .bind(now)
        .bind(&task_id)
        .bind(&root)
        .bind(owner)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if returned == 0 {
            return Err(refusal(
                &mut transaction,
                &root,
                &task_id,
                Some(owner),
                PlanStatus::Open,
            )
            .await);
        }
        settle_blocks(&mut transaction, &root, now).await?;
        finish(transaction, &root, &task_id).await
    }

    /// Forgets the member that lost `task`, which is open: the next claim gives it to a
    /// fresh member, or to that thread as a spare, with a card.
    pub async fn forget_last_owner(&self, thread: &str, task: &str) -> Result<(), PlanError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        sqlx::query(
            "UPDATE tasks SET last_owner_thread_id = NULL
             WHERE task_id = ? AND thread_id = ? AND status = 'open'",
        )
        .bind(&task_id)
        .bind(&root)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Releases every task of the plan whose lease ran out before `now`, as
    /// `release_task` does. Answers with them as they are afterwards.
    pub async fn release_expired(
        &self,
        thread: &str,
        now: i64,
    ) -> Result<Vec<PlanTask>, PlanError> {
        self.expired(thread, now, true).await
    }

    /// Returns every task of the plan whose lease ran out before `now` to open, as
    /// `return_task` does: the attempt is not counted and none fails. For a team that
    /// winds down, whose members the core told to stop.
    pub async fn return_expired(&self, thread: &str, now: i64) -> Result<Vec<PlanTask>, PlanError> {
        self.expired(thread, now, false).await
    }

    async fn expired(
        &self,
        thread: &str,
        now: i64,
        counted: bool,
    ) -> Result<Vec<PlanTask>, PlanError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let released = release(
            &mut transaction,
            &root,
            None,
            Some(now),
            "the lease ran out",
            now,
            counted,
        )
        .await?;
        settle_blocks(&mut transaction, &root, now).await?;
        finish_all(transaction, &root, &released).await
    }

    /// The team ended: tasks at work go back to open, whatever their attempts, and none
    /// fails. The attempt their claim counted is taken back: the end cut their members
    /// off, which was not the members' doing.
    pub async fn return_work(&self, thread: &str) -> Result<Vec<PlanTask>, PlanError> {
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let returned: Vec<String> = sqlx::query_scalar(
            "UPDATE tasks SET status = 'open', owner_thread_id = NULL, lease_until = NULL,
                 last_owner_thread_id = owner_thread_id,
                 attempts = MAX(attempts - 1, 0), updated_at = ?
             WHERE thread_id = ? AND status IN ('claimed', 'running') RETURNING task_id",
        )
        .bind(now)
        .bind(&root)
        .fetch_all(&mut *transaction)
        .await?;
        settle_blocks(&mut transaction, &root, now).await?;
        finish_all(transaction, &root, &returned).await
    }

    /// running to review: the member's turn completed, its answer is the hand-in.
    pub async fn hand_in(&self, owner: &str, task: &str) -> Result<PlanTask, PlanError> {
        self.leave_running(owner, task, PlanStatus::Review, None, "")
            .await
    }

    /// running to blocked: the member cannot go on. `waits_on` is what it waits for, when
    /// that is a task or a message; `note` is what it said it needs.
    pub async fn block_task(
        &self,
        owner: &str,
        task: &str,
        waits_on: Option<&BlockedOn>,
        note: &str,
    ) -> Result<PlanTask, PlanError> {
        self.leave_running(owner, task, PlanStatus::Blocked, waits_on, note)
            .await
    }

    async fn leave_running(
        &self,
        owner: &str,
        task: &str,
        to: PlanStatus,
        waits_on: Option<&BlockedOn>,
        note: &str,
    ) -> Result<PlanTask, PlanError> {
        capped("the note", note, MAX_NOTE)?;
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, owner).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let waits_on = match waits_on {
            None => None,
            Some(BlockedOn::Message(id)) => Some(WaitsOn::Message(*id)),
            Some(BlockedOn::Task(other)) => {
                Some(WaitsOn::Task(locate(&mut transaction, &root, other).await?))
            }
        };
        let moved = sqlx::query(
            "UPDATE tasks SET status = ?, lease_until = NULL, waits_on = ?, note = ?, updated_at = ?
             WHERE task_id = ? AND thread_id = ? AND owner_thread_id = ? AND status = 'running'",
        )
        .bind(to.as_str())
        .bind(waits_on.as_ref().map(WaitsOn::stored))
        .bind(note)
        .bind(now)
        .bind(&task_id)
        .bind(&root)
        .bind(owner)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if moved == 0 {
            return Err(refusal(&mut transaction, &root, &task_id, Some(owner), to).await);
        }
        finish(transaction, &root, &task_id).await
    }

    /// blocked to running: what the member waited for arrived, and it is woken. The lease
    /// starts again. A task blocked behind a failed one has no member to wake.
    pub async fn unblock_task(
        &self,
        thread: &str,
        task: &str,
        now: i64,
    ) -> Result<PlanTask, PlanError> {
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let moved = sqlx::query(
            "UPDATE tasks SET status = 'running', lease_until = ?, waits_on = NULL, updated_at = ?
             WHERE task_id = ? AND thread_id = ? AND status = 'blocked'
               AND owner_thread_id IS NOT NULL",
        )
        .bind(now.saturating_add(LEASE_MS))
        .bind(now)
        .bind(&task_id)
        .bind(&root)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if moved == 0 {
            return Err(
                refusal(&mut transaction, &root, &task_id, None, PlanStatus::Running).await,
            );
        }
        finish(transaction, &root, &task_id).await
    }

    /// A task in review that the gate may not pass alone (it changed a path outside its
    /// files): it waits for the lead, with `note` as the reason.
    pub async fn hold_for_lead(
        &self,
        thread: &str,
        task: &str,
        note: &str,
    ) -> Result<PlanTask, PlanError> {
        capped("the note", note, MAX_NOTE)?;
        let hold = "UPDATE tasks SET waits_on = 'lead', note = ?, updated_at = ?
                    WHERE task_id = ? AND thread_id = ? AND status = 'review'";
        self.in_review(thread, task, hold, note).await
    }

    /// Records the background job that runs the gate of a task in review.
    pub async fn set_gate_job(
        &self,
        thread: &str,
        task: &str,
        job_id: &str,
    ) -> Result<PlanTask, PlanError> {
        capped("the job id", job_id, MAX_MODEL)?;
        let record = "UPDATE tasks SET gate_job = ?, updated_at = ?
                      WHERE task_id = ? AND thread_id = ? AND status = 'review'";
        self.in_review(thread, task, record, job_id).await
    }

    /// Writes one value into a task that is in review and stays there.
    async fn in_review(
        &self,
        thread: &str,
        task: &str,
        update: &'static str,
        value: &str,
    ) -> Result<PlanTask, PlanError> {
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let written = sqlx::query(update)
            .bind(value)
            .bind(now)
            .bind(&task_id)
            .bind(&root)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        if written == 0 {
            return Err(refusal(&mut transaction, &root, &task_id, None, PlanStatus::Review).await);
        }
        finish(transaction, &root, &task_id).await
    }

    /// review to done. The gate passes a task with a command gate that is not held for
    /// the lead; the lead a task without a gate or one held for it; the user a task whose
    /// gate is its confirmation.
    pub async fn pass_task(
        &self,
        thread: &str,
        task: &str,
        by: PassedBy,
    ) -> Result<PlanTask, PlanError> {
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let found = load(&mut transaction, &root, &task_id).await?;
        if found.status != PlanStatus::Review {
            return Err(PlanError::Illegal {
                task: task_id,
                from: found.status,
                to: PlanStatus::Done,
            });
        }
        let held = found.waits_on == Some(WaitsOn::Lead);
        let reviewed = matches!(found.waits_on, Some(WaitsOn::Review(_)));
        let refused = match (by, &found.gate) {
            (PassedBy::Review, _) if reviewed => None,
            (PassedBy::Review, _) => Some("no review of it runs"),
            // A gate run twice, as after a stop, does not pass what a reviewer checks.
            (PassedBy::Gate, _) if reviewed => Some("a fresh review of it runs"),
            (PassedBy::Gate, Some(GoalCheck::Command { .. })) if !held => None,
            (PassedBy::Gate, Some(GoalCheck::Command { .. })) => {
                Some("it changed more than its files; the lead decides")
            }
            (PassedBy::Gate, _) => Some("it has no command for a gate"),
            (PassedBy::Lead, None) => None,
            (PassedBy::Lead, Some(_)) if held => None,
            (PassedBy::Lead, Some(GoalCheck::Command { .. })) => {
                Some("its gate is a command the core runs")
            }
            (PassedBy::Lead, Some(GoalCheck::UserConfirmation)) => Some("the user confirms it"),
            (PassedBy::User, Some(GoalCheck::UserConfirmation)) => None,
            (PassedBy::User, _) => Some("its gate is not the user's confirmation"),
        };
        if let Some(why) = refused {
            return Err(PlanError::Gate { task: task_id, why });
        }
        let moved = sqlx::query(
            "UPDATE tasks SET status = 'done', done = 1, waits_on = NULL, updated_at = ?
             WHERE task_id = ? AND thread_id = ? AND status = 'review'",
        )
        .bind(now)
        .bind(&task_id)
        .bind(&root)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if moved == 0 {
            return Err(refusal(&mut transaction, &root, &task_id, None, PlanStatus::Done).await);
        }
        finish(transaction, &root, &task_id).await
    }

    /// review back to running, with `note` as what to redo: the gate failed, the merge
    /// conflicted or the lead rejected. The same thread goes on, its lease starts again.
    /// A task that came back twice already fails instead.
    pub async fn rework_task(
        &self,
        thread: &str,
        task: &str,
        note: &str,
        now: i64,
    ) -> Result<PlanTask, PlanError> {
        capped("the note", note, MAX_NOTE)?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let moved = sqlx::query(
            "UPDATE tasks SET
                 status = CASE WHEN reworks < ?1 THEN 'running' ELSE 'failed' END,
                 lease_until = CASE WHEN reworks < ?1 THEN ?2 END,
                 reworks = MIN(reworks + 1, ?1), waits_on = NULL, note = ?3, updated_at = ?4
             WHERE task_id = ?5 AND thread_id = ?6 AND status = 'review'",
        )
        .bind(i64::from(MAX_REWORKS))
        .bind(now.saturating_add(LEASE_MS))
        .bind(note)
        .bind(now)
        .bind(&task_id)
        .bind(&root)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if moved == 0 {
            return Err(
                refusal(&mut transaction, &root, &task_id, None, PlanStatus::Running).await,
            );
        }
        settle_blocks(&mut transaction, &root, now).await?;
        finish(transaction, &root, &task_id).await
    }

    /// Any status but done, to failed: the lead's `fail`. Open tasks behind it block.
    pub async fn fail_task(
        &self,
        thread: &str,
        task: &str,
        note: &str,
    ) -> Result<PlanTask, PlanError> {
        capped("the note", note, MAX_NOTE)?;
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let moved = sqlx::query(
            "UPDATE tasks SET status = 'failed', lease_until = NULL, waits_on = NULL, note = ?,
                 updated_at = ?
             WHERE task_id = ? AND thread_id = ? AND status NOT IN ('done', 'failed')",
        )
        .bind(note)
        .bind(now)
        .bind(&task_id)
        .bind(&root)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if moved == 0 {
            return Err(refusal(&mut transaction, &root, &task_id, None, PlanStatus::Failed).await);
        }
        settle_blocks(&mut transaction, &root, now).await?;
        finish(transaction, &root, &task_id).await
    }

    /// The lead's `reopen` and the user's retry. Either way the last owner is forgotten:
    /// the lead tries again, maybe with a new brief, and a fresh member reads it from its
    /// card. failed to open, a retry in place: the id, the history and the dependents
    /// stay, the owner is cleared, `attempts` and `reworks` start again, `retries` counts
    /// one more, and tasks blocked or skipped behind it come back. blocked to open, for a
    /// task whose member is stuck: the owner is cleared. A task blocked behind a failed
    /// one is refused: it opens when that one is reopened.
    pub async fn reopen_task(&self, thread: &str, task: &str) -> Result<PlanTask, PlanError> {
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await?;
        let root = root_of(&mut transaction, thread).await?;
        let task_id = locate(&mut transaction, &root, task).await?;
        let found = load(&mut transaction, &root, &task_id).await?;
        if let (PlanStatus::Blocked, None, Some(WaitsOn::Task(dependency))) =
            (found.status, &found.owner_thread_id, &found.waits_on)
        {
            return Err(PlanError::BehindFailed {
                task: task_id,
                dependency: dependency.clone(),
            });
        }
        let moved = sqlx::query(
            "UPDATE tasks SET
                 attempts = CASE status WHEN 'failed' THEN 0 ELSE attempts END,
                 reworks = CASE status WHEN 'failed' THEN 0 ELSE reworks END,
                 retries = CASE status WHEN 'failed' THEN retries + 1 ELSE retries END,
                 retried_at = CASE status WHEN 'failed' THEN ?1 ELSE retried_at END,
                 status = 'open', owner_thread_id = NULL, waits_on = NULL, note = '',
                 last_owner_thread_id = NULL, skipped = 0, updated_at = ?1
             WHERE task_id = ?2 AND thread_id = ?3 AND status IN ('failed', 'blocked')",
        )
        .bind(now)
        .bind(&task_id)
        .bind(&root)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if moved == 0 {
            return Err(refusal(&mut transaction, &root, &task_id, None, PlanStatus::Open).await);
        }
        // What a skip of it gave up comes back with it.
        if found.skipped {
            mark_behind(&mut transaction, &root, &task_id, false, now).await?;
        }
        settle_blocks(&mut transaction, &root, now).await?;
        finish(transaction, &root, &task_id).await
    }
}

/// Takes tasks at work back: one by id, or all whose lease ended before `expired`.
/// claimed or running back to open, or to failed at `MAX_ATTEMPTS`.
/// The owner is kept as the last owner, which may be given the task back, unless its
/// lease ran out: a member that hung, which the core cuts off, is not. Not `counted`, the
/// attempt the claim counted is taken back and none fails.
async fn release(
    transaction: &mut Tx<'_>,
    root: &str,
    task_id: Option<&str>,
    expired: Option<i64>,
    note: &str,
    now: i64,
    counted: bool,
) -> Result<Vec<String>, PlanError> {
    Ok(sqlx::query_scalar(
        "UPDATE tasks SET
             status = CASE WHEN ?7 AND attempts >= ?1 THEN 'failed' ELSE 'open' END,
             owner_thread_id = CASE WHEN ?7 AND attempts >= ?1 THEN owner_thread_id END,
             last_owner_thread_id = CASE WHEN ?6 IS NULL THEN owner_thread_id END,
             attempts = CASE WHEN ?7 THEN attempts ELSE MAX(attempts - 1, 0) END,
             lease_until = NULL, note = ?2, updated_at = ?3
         WHERE thread_id = ?4 AND status IN ('claimed', 'running')
           AND (?5 IS NULL OR task_id = ?5) AND (?6 IS NULL OR lease_until < ?6)
         RETURNING task_id",
    )
    .bind(i64::from(MAX_ATTEMPTS))
    .bind(note)
    .bind(now)
    .bind(root)
    .bind(task_id)
    .bind(expired)
    .bind(counted)
    .fetch_all(&mut **transaction)
    .await?)
}

/// Commits and answers with the task as it is now.
async fn finish(mut transaction: Tx<'_>, root: &str, task_id: &str) -> Result<PlanTask, PlanError> {
    let task = load(&mut transaction, root, task_id).await?;
    transaction.commit().await?;
    Ok(task)
}

async fn finish_all(
    mut transaction: Tx<'_>,
    root: &str,
    ids: &[String],
) -> Result<Vec<PlanTask>, PlanError> {
    let mut tasks = Vec::with_capacity(ids.len());
    for task_id in ids {
        tasks.push(load(&mut transaction, root, task_id).await?);
    }
    transaction.commit().await?;
    Ok(tasks)
}

/// Why a move matched no row: the task is another thread's, or its status has no such
/// move. The transaction is dropped by the caller, so nothing it wrote stays.
async fn refusal(
    transaction: &mut Tx<'_>,
    root: &str,
    task_id: &str,
    owner: Option<&str>,
    to: PlanStatus,
) -> PlanError {
    match load(transaction, root, task_id).await {
        Err(error) => error,
        Ok(task) if owner.is_some() && task.owner_thread_id.as_deref() != owner => {
            PlanError::NotOwner { task: task.task_id }
        }
        Ok(task) => PlanError::Illegal {
            task: task.task_id,
            from: task.status,
            to,
        },
    }
}

/// Which condition of the claim did not hold, asked in the claim's own order.
async fn why_not_claimed(
    transaction: &mut Tx<'_>,
    root: &str,
    task_id: &str,
    claim: Claim<'_>,
) -> Result<PlanError, PlanError> {
    let task = load(transaction, root, task_id).await?;
    if task.status != PlanStatus::Open {
        return Ok(PlanError::Illegal {
            task: task.task_id,
            from: task.status,
            to: PlanStatus::Claimed,
        });
    }
    let not_done: Option<String> = sqlx::query_scalar(
        "SELECT d.value FROM tasks t, json_each(t.depends_on) d
         LEFT JOIN tasks dep ON dep.task_id = d.value AND dep.thread_id = t.thread_id
         WHERE t.task_id = ? AND dep.status IS NOT 'done' LIMIT 1",
    )
    .bind(task_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let held: Option<(String, String)> = sqlx::query_as(concat!(
        "SELECT o.task_id, theirs.value
         FROM tasks t, json_each(t.files) mine, tasks o, json_each(o.files) theirs
         WHERE t.task_id = ? AND o.thread_id = t.thread_id AND o.task_id != t.task_id AND ",
        holds_files!(),
        " AND ",
        overlap!(),
        " LIMIT 1"
    ))
    .bind(task_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let at_work: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE thread_id = ? AND status IN ('claimed', 'running')",
    )
    .bind(root)
    .fetch_one(&mut **transaction)
    .await?;
    let why = if let Some(dependency) = not_done {
        Unclaimable::Dependency(dependency)
    } else if let Some((holder, entry)) = held {
        Unclaimable::Files { holder, entry }
    } else if at_work >= i64::from(claim.at_once) {
        Unclaimable::AtOnce(claim.at_once)
    } else {
        Unclaimable::OwnerBusy
    };
    Ok(PlanError::NotClaimable {
        task: task.task_id,
        why,
    })
}
