//! Setting, judging, pausing and clearing a goal. Only the user's `/goal` sets or
//! clears one by hand; the runner counts the turns judged and pauses or clears it.

use anyhow::{Context, Result, ensure};
use dusk_protocol::thread::MAX_GOAL_CHARS;

use super::{Goal, GoalStatus, State, decode, timestamp_ms};
use crate::state::thread::threads::family;

impl State {
    /// The goal with what the chat and its agents spent since it started.
    pub async fn goal(&self, thread_id: &str) -> Result<Option<Goal>> {
        let row = sqlx::query(family!(
            "SELECT goals.*, (SELECT COALESCE(SUM(prompt_tokens + completion_tokens), 0)
                FROM model_usage WHERE thread_id IN (SELECT id FROM family)
                AND created_at >= goals.started_at) AS spent
             FROM goals WHERE thread_id = ?1"
        ))
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await
        .context("read goal")?;
        row.map(decode).transpose()
    }

    /// Replaces the chat's goal with a new active one.
    pub async fn set_goal(&self, thread_id: &str, text: &str) -> Result<Goal> {
        let text = text.trim();
        ensure!(!text.is_empty(), "the goal is empty");
        ensure!(
            text.chars().count() <= MAX_GOAL_CHARS,
            "the goal is longer than {MAX_GOAL_CHARS} characters"
        );
        let now = timestamp_ms()?;
        sqlx::query(
            "INSERT INTO goals (thread_id, text, status, rounds, note, updated_at, started_at)
             VALUES (?1, ?2, 'active', 0, NULL, ?3, ?3)
             ON CONFLICT (thread_id) DO UPDATE SET text = excluded.text, status = 'active',
                rounds = 0, note = NULL, reason = NULL, awaiting_user = 0,
                updated_at = excluded.updated_at, started_at = excluded.started_at",
        )
        .bind(thread_id)
        .bind(text)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("store goal")?;
        self.goal(thread_id).await?.context("goal is gone")
    }

    /// Pauses a goal with why, or makes it active again.
    pub async fn set_goal_status(
        &self,
        thread_id: &str,
        status: GoalStatus,
        note: Option<&str>,
    ) -> Result<Option<Goal>> {
        let note = note.map(str::trim).filter(|note| !note.is_empty());
        sqlx::query("UPDATE goals SET status = ?, note = ?, updated_at = ? WHERE thread_id = ?")
            .bind(status.name())
            .bind(note)
            .bind(timestamp_ms()?)
            .bind(thread_id)
            .execute(&self.pool)
            .await
            .context("change goal status")?;
        self.goal(thread_id).await
    }

    /// Counts a turn the evaluator judged and keeps its reason.
    pub async fn count_goal_turn(&self, thread_id: &str, reason: &str) -> Result<Option<Goal>> {
        sqlx::query(
            "UPDATE goals SET rounds = rounds + 1, reason = ?, updated_at = ? WHERE thread_id = ?",
        )
        .bind(reason)
        .bind(timestamp_ms()?)
        .bind(thread_id)
        .execute(&self.pool)
        .await
        .context("count goal turn")?;
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

    /// When the app starts, every goal keeps its condition and state, and its turns,
    /// time and spend count from now.
    pub async fn restore_goals(&self) -> Result<()> {
        sqlx::query("UPDATE goals SET rounds = 0, started_at = ?")
            .bind(timestamp_ms()?)
            .execute(&self.pool)
            .await
            .context("restore goals")?;
        Ok(())
    }

    /// The commands that passed in the chat's turns since `since`, newest first, for the
    /// episode of a goal met.
    pub async fn goal_commands_since(&self, thread_id: &str, since: i64) -> Result<Vec<String>> {
        sqlx::query_scalar(
            "SELECT command FROM (SELECT json_extract(e.arguments_json, '$.command') AS command,
                    MAX(e.rowid) AS newest
                FROM tool_executions e JOIN turns t ON t.turn_id = e.turn_id
                WHERE e.thread_id = ? AND e.tool_name = 'run_command' AND e.outcome = 'succeeded'
                    AND t.started_at >= ?
                    AND json_extract(e.arguments_json, '$.command') IS NOT NULL
                GROUP BY command)
             ORDER BY newest DESC LIMIT 10",
        )
        .bind(thread_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .context("read the goal's commands")
    }
}
