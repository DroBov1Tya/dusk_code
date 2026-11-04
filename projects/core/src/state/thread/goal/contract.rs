//! The goal's contract: the user sets it, the agent proposes criteria for a draft, the
//! user approves them, pauses, resumes or clears it, and each round is counted.

use anyhow::{Context, Result, ensure};
use uuid::Uuid;

use super::{Goal, GoalCriterion, GoalStatus, State, decode, timestamp_ms};

impl State {
    pub async fn goal(&self, thread_id: &str) -> Result<Option<Goal>> {
        let row = sqlx::query("SELECT * FROM goals WHERE thread_id = ?")
            .bind(thread_id)
            .fetch_optional(&self.pool)
            .await
            .context("read goal")?;
        row.map(decode).transpose()
    }

    /// Only user commands replace a goal. A fresh revision invalidates all evidence.
    pub async fn set_goal(&self, thread_id: &str, text: &str) -> Result<Goal> {
        ensure!(!text.trim().is_empty(), "the goal is empty");
        sqlx::query(
            "INSERT INTO goals (thread_id, text, status, rounds, note, updated_at, revision)
             VALUES (?, ?, 'paused', 0, NULL, ?, ?)
             ON CONFLICT (thread_id) DO UPDATE SET text = excluded.text,
                status = 'paused', rounds = 0, note = NULL, updated_at = excluded.updated_at,
                revision = excluded.revision, criteria_json = '[]', approved_at = NULL, work_epoch = 0, awaiting_user = 0,
                evidence_after_sequence = 0, verification = 'unverified', verification_id = NULL,
                verification_request = NULL, verification_turn_id = NULL,
                verification_attempts = 0, verification_note = NULL",
        )
        .bind(thread_id)
        .bind(text.trim())
        .bind(timestamp_ms()?)
        .bind(Uuid::now_v7().to_string())
        .execute(&self.pool)
        .await
        .context("store draft goal")?;
        self.goal(thread_id).await?.context("goal is gone")
    }

    /// Proposal is allowed only before approval; it receives a new approval token.
    pub async fn propose_goal(
        &self,
        thread_id: &str,
        revision: &str,
        criteria: Vec<GoalCriterion>,
    ) -> Result<Goal> {
        GoalCriterion::validate(&criteria)?;
        let changed = sqlx::query(
            "UPDATE goals SET criteria_json = ?, revision = ?, updated_at = ?
            WHERE thread_id = ? AND revision = ? AND approved_at IS NULL",
        )
        .bind(serde_json::to_string(&criteria)?)
        .bind(Uuid::now_v7().to_string())
        .bind(timestamp_ms()?)
        .bind(thread_id)
        .bind(revision)
        .execute(&self.pool)
        .await?;
        ensure!(
            changed.rows_affected() == 1,
            "contract changed or is approved; only the user can replace it"
        );
        self.goal(thread_id).await?.context("goal is gone")
    }

    pub async fn approve_goal(&self, thread_id: &str, revision: &str) -> Result<Goal> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT * FROM goals WHERE thread_id = ? AND revision = ?")
            .bind(thread_id)
            .bind(revision)
            .fetch_optional(&mut *tx)
            .await?
            .context("goal revision changed; review /goal again")?;
        let goal = decode(row)?;
        ensure!(!goal.approved, "contract is already approved");
        GoalCriterion::validate(&goal.criteria)?;
        sqlx::query("UPDATE goals SET approved_at = ?, status = 'active', rounds = 0,
            evidence_after_sequence = (SELECT COALESCE(MAX(sequence), 0) FROM messages WHERE thread_id = ?),
            updated_at = ? WHERE thread_id = ? AND revision = ?")
            .bind(timestamp_ms()?).bind(thread_id).bind(timestamp_ms()?).bind(thread_id).bind(revision)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        self.goal(thread_id).await?.context("goal is gone")
    }

    /// Public transitions cannot complete a goal. Runtime verification owns Done.
    pub async fn set_goal_status(
        &self,
        thread_id: &str,
        status: GoalStatus,
        note: Option<&str>,
    ) -> Result<Option<Goal>> {
        ensure!(
            status != GoalStatus::Done,
            "only runtime verification can complete a goal"
        );
        let note = note.map(str::trim).filter(|note| !note.is_empty());
        let changed = sqlx::query(
            "UPDATE goals SET status = ?, note = ?, updated_at = ?,
                rounds = CASE WHEN ? = 'active' THEN 0 ELSE rounds END,
                work_epoch = work_epoch + CASE WHEN ? = 'active' THEN 1 ELSE 0 END,
                verification = CASE WHEN verification = 'passed' THEN 'unverified' ELSE verification END,
                awaiting_user = CASE WHEN ? = 'active' THEN 0 ELSE awaiting_user END,
                verification_attempts = CASE WHEN ? = 'active' THEN 0 ELSE verification_attempts END
             WHERE thread_id = ? AND (? != 'active' OR approved_at IS NOT NULL)",
        ).bind(status.name()).bind(note).bind(timestamp_ms()?).bind(status.name()).bind(status.name()).bind(status.name()).bind(status.name())
            .bind(thread_id).bind(status.name()).execute(&self.pool).await?;
        ensure!(
            changed.rows_affected() == 1,
            "no approved goal; review and approve its criteria first"
        );
        self.goal(thread_id).await
    }

    pub async fn count_goal_round(&self, thread_id: &str) -> Result<Option<Goal>> {
        sqlx::query("UPDATE goals SET rounds = rounds + 1, updated_at = ? WHERE thread_id = ?")
            .bind(timestamp_ms()?)
            .bind(thread_id)
            .execute(&self.pool)
            .await
            .context("count goal round")?;
        self.goal(thread_id).await
    }

    /// Whether there was a goal to clear.
    pub async fn clear_goal(&self, thread_id: &str) -> Result<bool> {
        let done = sqlx::query("DELETE FROM goals WHERE thread_id = ?")
            .bind(thread_id)
            .execute(&self.pool)
            .await
            .context("clear goal")?;
        Ok(done.rows_affected() > 0)
    }
}
