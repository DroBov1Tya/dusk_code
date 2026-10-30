//! A thread's standing goal, kept across restarts.

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use super::{State, timestamp_ms};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStatus {
    Active,
    Paused,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Goal {
    pub thread_id: String,
    pub text: String,
    pub status: GoalStatus,
    pub rounds: u32,
    pub note: Option<String>,
}

impl GoalStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Done => "done",
        }
    }

    fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "active" => Self::Active,
            "paused" => Self::Paused,
            "done" => Self::Done,
            other => bail!("unknown goal status {other}"),
        })
    }
}

impl State {
    pub async fn goal(&self, thread_id: &str) -> Result<Option<Goal>> {
        let row = sqlx::query("SELECT * FROM goals WHERE thread_id = ?")
            .bind(thread_id)
            .fetch_optional(&self.pool)
            .await
            .context("read goal")?;
        row.map(decode).transpose()
    }

    /// A new goal replaces the old one and starts active with no rounds counted.
    pub async fn set_goal(&self, thread_id: &str, text: &str) -> Result<Goal> {
        ensure!(!text.trim().is_empty(), "the goal is empty");
        sqlx::query(
            "INSERT INTO goals (thread_id, text, status, rounds, note, updated_at)
             VALUES (?, ?, 'active', 0, NULL, ?)
             ON CONFLICT (thread_id) DO UPDATE SET text = excluded.text,
                status = 'active', rounds = 0, note = NULL, updated_at = excluded.updated_at",
        )
        .bind(thread_id)
        .bind(text.trim())
        .bind(timestamp_ms()?)
        .execute(&self.pool)
        .await
        .context("store goal")?;
        self.goal(thread_id).await?.context("goal is gone")
    }

    /// Resuming starts the rounds over.
    pub async fn set_goal_status(
        &self,
        thread_id: &str,
        status: GoalStatus,
        note: Option<&str>,
    ) -> Result<Option<Goal>> {
        let note = note.map(str::trim).filter(|note| !note.is_empty());
        sqlx::query(
            "UPDATE goals SET status = ?, note = ?, updated_at = ?,
                rounds = CASE WHEN ? = 'active' THEN 0 ELSE rounds END
             WHERE thread_id = ?",
        )
        .bind(status.name())
        .bind(note)
        .bind(timestamp_ms()?)
        .bind(status.name())
        .bind(thread_id)
        .execute(&self.pool)
        .await
        .context("change goal")?;
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

fn decode(row: SqliteRow) -> Result<Goal> {
    let rounds: i64 = row.try_get("rounds")?;
    Ok(Goal {
        thread_id: row.try_get("thread_id")?,
        text: row.try_get("text")?,
        status: GoalStatus::parse(row.try_get("status")?)?,
        rounds: u32::try_from(rounds).context("goal rounds out of range")?,
        note: row.try_get("note")?,
    })
}
