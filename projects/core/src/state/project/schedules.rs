//! Scheduled tasks as rows. When a schedule runs next is worked out by
//! `project::schedule`; this file only keeps what it is told.

use anyhow::{Context, Result};
use sqlx::{Row, sqlite::SqliteRow};

use crate::state::{State, timestamp_ms};

pub use dusk_protocol::project::Schedule;

impl State {
    /// Every schedule, soonest first; the ones that will not run again last.
    pub async fn list_schedules(&self) -> Result<Vec<Schedule>> {
        sqlx::query(
            "SELECT schedules.*, schedule_models.model FROM schedules
             LEFT JOIN schedule_models ON schedule_models.schedule_id = schedules.id
             ORDER BY next_run_at IS NULL, next_run_at, created_at",
        )
        .try_map(schedule_from_row)
        .fetch_all(&self.pool)
        .await
        .context("list the scheduled tasks")
    }

    pub async fn load_schedule(&self, id: &str) -> Result<Option<Schedule>> {
        sqlx::query(
            "SELECT schedules.*, schedule_models.model FROM schedules
             LEFT JOIN schedule_models ON schedule_models.schedule_id = schedules.id
             WHERE id = ?",
        )
        .bind(id)
        .try_map(schedule_from_row)
        .fetch_optional(&self.pool)
        .await
        .context("read the scheduled task")
    }

    /// The enabled schedules whose time has come.
    pub async fn due_schedules(&self, now_ms: i64) -> Result<Vec<Schedule>> {
        sqlx::query(
            "SELECT schedules.*, schedule_models.model FROM schedules
             LEFT JOIN schedule_models ON schedule_models.schedule_id = schedules.id
             WHERE enabled = 1 AND next_run_at IS NOT NULL AND next_run_at <= ?
             ORDER BY next_run_at",
        )
        .bind(now_ms)
        .try_map(schedule_from_row)
        .fetch_all(&self.pool)
        .await
        .context("find the scheduled tasks that are due")
    }

    pub async fn count_schedules(&self) -> Result<usize> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schedules")
            .fetch_one(&self.pool)
            .await
            .context("count the scheduled tasks")?;
        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }

    /// Writes the schedule as it is, new or changed.
    pub async fn save_schedule(&self, schedule: &Schedule) -> Result<()> {
        let repeat = serde_json::to_string(&schedule.repeat)?;
        sqlx::query(
            "INSERT INTO schedules (id, project_id, prompt, repeat, enabled, next_run_at,
                 last_run_at, last_thread_id, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET project_id = excluded.project_id,
                 prompt = excluded.prompt, repeat = excluded.repeat,
                 enabled = excluded.enabled, next_run_at = excluded.next_run_at,
                 last_run_at = excluded.last_run_at, last_thread_id = excluded.last_thread_id",
        )
        .bind(&schedule.id)
        .bind(&schedule.project_id)
        .bind(&schedule.prompt)
        .bind(repeat)
        .bind(schedule.enabled)
        .bind(schedule.next_run_at)
        .bind(schedule.last_run_at)
        .bind(&schedule.last_thread_id)
        .bind(timestamp_ms()?)
        .execute(&self.pool)
        .await
        .context("save the scheduled task; does its project exist?")?;
        match &schedule.model {
            Some(model) => sqlx::query(
                "INSERT INTO schedule_models (schedule_id, model) VALUES (?, ?)
                 ON CONFLICT (schedule_id) DO UPDATE SET model = excluded.model",
            )
            .bind(&schedule.id)
            .bind(model),
            None => {
                sqlx::query("DELETE FROM schedule_models WHERE schedule_id = ?").bind(&schedule.id)
            }
        }
        .execute(&self.pool)
        .await
        .context("save the task's model")?;
        Ok(())
    }

    pub async fn delete_schedule(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("remove the scheduled task")?;
        Ok(())
    }
}

fn schedule_from_row(row: SqliteRow) -> Result<Schedule, sqlx::Error> {
    let repeat: String = row.try_get("repeat")?;
    Ok(Schedule {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        prompt: row.try_get("prompt")?,
        repeat: serde_json::from_str(&repeat).map_err(|error| sqlx::Error::Decode(error.into()))?,
        enabled: row.try_get("enabled")?,
        next_run_at: row.try_get("next_run_at")?,
        last_run_at: row.try_get("last_run_at")?,
        last_thread_id: row.try_get("last_thread_id")?,
        model: row.try_get("model")?,
    })
}
