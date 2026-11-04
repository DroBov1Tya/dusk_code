//! Whether a goal is met: the saved results the agent submits as evidence, the user's
//! confirmations, and what the runtime waits on before it reviews them.

use anyhow::{Context, Result, bail, ensure};
use uuid::Uuid;

use super::{Goal, GoalCheck, GoalEvidence, State, decode, timestamp_ms};

impl State {
    /// Pending input and durable dependencies in one snapshot. Agent requests may be
    /// waiting for a slot, admitted but not started yet, or in a running turn.
    pub async fn goal_wake_state(&self, thread_id: &str) -> Result<(bool, bool)> {
        sqlx::query_as(
            "WITH RECURSIVE agents(id) AS (
                SELECT id FROM threads WHERE parent_thread_id = ?1
                UNION ALL
                SELECT threads.id FROM threads JOIN agents ON threads.parent_thread_id = agents.id
             ) SELECT EXISTS(SELECT 1 FROM pending_messages WHERE thread_id = ?1),
                EXISTS(SELECT 1 FROM background_jobs WHERE thread_id = ?1 AND status = 'running')
                OR EXISTS(SELECT 1 FROM pending_messages JOIN agents ON thread_id = agents.id)
                OR EXISTS(SELECT 1 FROM turns JOIN agents ON thread_id = agents.id
                    WHERE status = 'running')
                OR EXISTS(SELECT 1 FROM goals WHERE thread_id = ?1 AND awaiting_user = 1)
                OR EXISTS(SELECT 1 FROM messages JOIN agents ON thread_id = agents.id
                    WHERE (role = 'user' OR from_job_id IS NOT NULL)
                    AND sequence > (SELECT COALESCE(MAX(last_sequence), 0) FROM turns
                        WHERE turns.thread_id = agents.id))",
        )
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await
        .context("check goal dependency wake state")
    }

    pub async fn request_goal_verification(
        &self,
        thread_id: &str,
        revision: &str,
        call_id: &str,
        evidence: Vec<GoalEvidence>,
    ) -> Result<Goal> {
        let mut tx = self.pool.begin().await?;
        let goal = sqlx::query(
            "SELECT * FROM goals WHERE thread_id = ? AND revision = ?
            AND status = 'active' AND approved_at IS NOT NULL",
        )
        .bind(thread_id)
        .bind(revision)
        .fetch_optional(&mut *tx)
        .await?
        .context("no active approved contract at this revision")?;
        let goal = decode(goal)?;
        ensure!(
            evidence.len() == goal.criteria.len(),
            "submit exactly one saved result for every criterion"
        );
        for criterion in &goal.criteria {
            ensure!(
                evidence
                    .iter()
                    .filter(|e| e.criterion_id == criterion.id)
                    .count()
                    == 1,
                "missing or duplicate evidence for {}",
                criterion.id
            );
        }
        ensure!(
            evidence.iter().all(|e| e.result_id.len() <= 128),
            "invalid result ID"
        );
        let turn: String = sqlx::query_scalar(
            "SELECT e.turn_id FROM tool_executions e JOIN turns t USING(turn_id)
            WHERE e.thread_id = ? AND e.call_id = ? AND e.tool_name = 'goal'
                AND e.goal_revision = ? AND t.status = 'running'",
        )
        .bind(thread_id)
        .bind(call_id)
        .bind(revision)
        .fetch_optional(&mut *tx)
        .await?
        .context("verification must be requested by a recorded call in the current goal turn")?;
        let attempts: i64 =
            sqlx::query_scalar("SELECT verification_attempts FROM goals WHERE thread_id = ?")
                .bind(thread_id)
                .fetch_one(&mut *tx)
                .await?;
        ensure!(
            attempts < 3,
            "three verification attempts used; user must review /goal and resume"
        );
        sqlx::query("UPDATE goals SET verification = 'pending', verification_id = ?,
            verification_request = ?, verification_turn_id = ?, verification_note = NULL WHERE thread_id = ?")
            .bind(Uuid::now_v7().to_string()).bind(serde_json::to_string(&evidence)?).bind(turn)
            .bind(thread_id).execute(&mut *tx).await?;
        tx.commit().await?;
        self.goal(thread_id).await?.context("goal is gone")
    }

    pub async fn confirm_goal(
        &self,
        thread_id: &str,
        revision: &str,
        criterion_id: &str,
        result_id: &str,
    ) -> Result<Goal> {
        let mut tx = self.pool.begin().await?;
        let goal = sqlx::query(
            "SELECT * FROM goals WHERE thread_id = ? AND revision = ? AND approved_at IS NOT NULL",
        )
        .bind(thread_id)
        .bind(revision)
        .fetch_optional(&mut *tx)
        .await?
        .context("approved revision changed; review /goal again")?;
        let goal = decode(goal)?;
        ensure!(
            goal.criteria
                .iter()
                .any(|c| c.id == criterion_id && c.check == GoalCheck::UserConfirmation),
            "criterion is not a user confirmation"
        );
        ensure!(
            goal.evidence
                .iter()
                .any(|e| e.criterion_id == criterion_id && e.result_id == result_id),
            "confirm the exact saved result currently submitted for this criterion"
        );
        sqlx::query("INSERT OR IGNORE INTO goal_confirmations VALUES (?, ?, ?, ?, ?)")
            .bind(thread_id)
            .bind(revision)
            .bind(criterion_id)
            .bind(result_id)
            .bind(timestamp_ms()?)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE goals SET verification = 'pending', verification_attempts = 0 WHERE thread_id = ?")
            .bind(thread_id).execute(&mut *tx).await?;
        tx.commit().await?;
        self.goal(thread_id).await?.context("goal is gone")
    }

    pub async fn recover_goal_verification(&self, thread_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE goals SET verification = 'gaps', verification_note =
            'Verification was interrupted; no pass was recorded. Submit fresh verification.'
            WHERE thread_id = ? AND verification IN ('checking', 'pending')",
        )
        .bind(thread_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn inspect_goal_result(&self, thread_id: &str, id: &str) -> Result<String> {
        let output: Option<String> = if let Some(id) = id.strip_prefix("tool:") {
            sqlx::query_scalar("SELECT 'tool=' || e.tool_name || ', outcome=' || e.outcome || ', exit=' || COALESCE(e.exit_code, 'unknown') ||
                    ', epoch=' || e.goal_epoch || ', current_epoch=' || g.work_epoch || ', arguments=' || e.arguments_json || char(10) || m.content
                FROM tool_executions e JOIN messages m ON m.message_id = e.result_message_id
                JOIN goals g ON g.thread_id = e.thread_id AND g.revision = e.goal_revision
                WHERE e.thread_id = ? AND e.result_message_id = ?")
                .bind(thread_id).bind(id).fetch_optional(&self.pool).await?
        } else if let Some(id) = id.strip_prefix("job:") {
            sqlx::query_scalar("SELECT 'status=' || b.status || ', command=' || b.command || ', epoch=' || b.goal_epoch ||
                ', current_epoch=' || g.work_epoch || char(10) || b.output FROM background_jobs b JOIN goals g
                ON g.thread_id = b.thread_id AND g.revision = b.goal_revision WHERE b.thread_id = ? AND b.job_id = ?")
                .bind(thread_id).bind(id).fetch_optional(&self.pool).await?
        } else {
            bail!("use a saved tool:<message ID> or job:<job ID>");
        };
        let output = output.context("no saved result for this goal revision and thread")?;
        Ok(format!("{id}\n{output}"))
    }

    /// Bounded index for both the user and model; text itself remains in message history.
    pub async fn goal_result_index(&self, thread_id: &str) -> Result<String> {
        let rows: Vec<(String, String, String, Option<i64>)> = sqlx::query_as(
            "SELECT 'tool:' || result_message_id, tool_name || ' ' || substr(arguments_json, 1, 300),
                outcome || CASE WHEN goal_epoch != (SELECT work_epoch FROM goals WHERE thread_id = ?1) THEN ' (stale)' ELSE '' END, exit_code FROM tool_executions
             WHERE thread_id = ?1 AND goal_revision = (SELECT revision FROM goals WHERE thread_id = ?1)
                AND result_message_id IS NOT NULL ORDER BY rowid DESC LIMIT 20")
            .bind(thread_id).fetch_all(&self.pool).await?;
        let jobs: Vec<(String, String, String)> = sqlx::query_as("SELECT 'job:' || job_id, command, status
            FROM background_jobs WHERE thread_id = ?1 AND goal_revision = (SELECT revision FROM goals WHERE thread_id = ?1)
            ORDER BY created_at DESC LIMIT 20").bind(thread_id).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|(id, tool, outcome, code)| format!("{id}: {tool} {outcome}, exit={code:?}"))
            .chain(
                jobs.into_iter()
                    .map(|(id, command, status)| format!("{id}: {status}, command={command}")),
            )
            .collect::<Vec<_>>()
            .join("\n"))
    }
}
