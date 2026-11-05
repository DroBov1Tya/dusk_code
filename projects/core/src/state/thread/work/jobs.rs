//! Durable job outcomes and the completion events waiting for their owning thread.

use anyhow::{Context, Result, ensure};
use sqlx::Row;

use super::{State, timestamp_ms};

#[derive(Debug, Clone)]
pub struct BackgroundJob {
    pub job_id: String,
    pub command: String,
    pub status: String,
    pub output: String,
    pub owner_runtime_id: Option<String>,
    pub owner_lease_path: Option<String>,
}

/// A job without its output, for lists that are read again and again.
#[derive(Debug, Clone)]
pub struct JobSummary {
    pub job_id: String,
    pub command: String,
    pub status: String,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub owner_runtime_id: Option<String>,
    pub owner_lease_path: Option<String>,
}

impl State {
    #[cfg(test)]
    pub async fn create_job(
        &self,
        thread_id: &str,
        job_id: &str,
        command: &str,
        runtime_id: &str,
        lease_path: &str,
    ) -> Result<()> {
        self.create_job_for_call(thread_id, job_id, command, runtime_id, lease_path, None)
            .await
    }

    /// The launch inherits the recorded call's epoch, including its position in a
    /// batch. Reading the goal's newest epoch could bless a check before a later edit.
    pub async fn create_job_for_call(
        &self,
        thread_id: &str,
        job_id: &str,
        command: &str,
        runtime_id: &str,
        lease_path: &str,
        call_id: Option<&str>,
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await.context("begin background job")?;
        let provenance: Option<(Option<String>, Option<i64>)> = if let Some(call_id) = call_id {
            let provenance = sqlx::query_as(
                "SELECT e.goal_revision, e.goal_epoch FROM tool_executions e
                JOIN turns t ON t.turn_id = e.turn_id AND t.thread_id = e.thread_id
                WHERE e.thread_id = ? AND e.call_id = ? AND e.tool_name = 'run_command'
                    AND e.outcome = 'pending' AND t.status = 'running'
                    AND json_extract(e.arguments_json, '$.command') = ?
                    AND json_extract(e.arguments_json, '$.background') = 1",
            )
            .bind(thread_id)
            .bind(call_id)
            .bind(command)
            .fetch_optional(&mut *transaction)
            .await?;
            ensure!(
                provenance.is_some(),
                "background job launch has no matching recorded call"
            );
            provenance
        } else {
            // Trusted runtime callers without a model tool call (including recovery tests).
            sqlx::query_as("SELECT revision, work_epoch FROM goals WHERE thread_id = ? AND approved_at IS NOT NULL")
                .bind(thread_id).fetch_optional(&mut *transaction).await?
        };
        let (goal_revision, goal_epoch) = provenance.unwrap_or_default();
        // Only finished history is pruned. Its completion event/message keeps the result.
        sqlx::query(
            "DELETE FROM background_jobs WHERE status != 'running' AND job_id NOT IN
             (SELECT job_id FROM background_jobs WHERE thread_id = ? ORDER BY created_at DESC, job_id DESC LIMIT 31)
             AND thread_id = ? AND NOT EXISTS (SELECT 1 FROM goals WHERE goals.thread_id = background_jobs.thread_id AND goals.revision = background_jobs.goal_revision)",
        )
        .bind(thread_id).bind(thread_id).execute(&mut *transaction).await?;
        sqlx::query(
            "DELETE FROM background_jobs WHERE status != 'running' AND job_id NOT IN
             (SELECT job_id FROM background_jobs ORDER BY created_at DESC, job_id DESC LIMIT 127)
             AND NOT EXISTS (SELECT 1 FROM goals WHERE goals.thread_id = background_jobs.thread_id
                AND goals.revision = background_jobs.goal_revision)",
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO background_jobs (job_id, thread_id, command, status, created_at, owner_runtime_id, owner_lease_path, goal_revision, goal_epoch)
             VALUES (?, ?, ?, 'running', ?, ?, ?, ?, ?)",
        )
        .bind(job_id)
        .bind(thread_id)
        .bind(command)
        .bind(timestamp_ms()?)
        .bind(runtime_id)
        .bind(lease_path)
        .bind(goal_revision)
        .bind(goal_epoch)
        .execute(&mut *transaction)
        .await
        .context("store background job")?;
        transaction.commit().await.context("commit background job")
    }

    pub async fn remove_unstarted_job(&self, job_id: &str, runtime_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM background_jobs WHERE job_id = ? AND owner_runtime_id = ? AND status = 'running'")
            .bind(job_id)
            .bind(runtime_id)
            .execute(&self.pool)
            .await
            .context("remove unstarted job")?;
        Ok(())
    }

    /// Outcome and notification are committed together, at most once per job. A replay
    /// after admission cannot enqueue another event, because the status is already final.
    pub async fn finish_job(
        &self,
        job_id: &str,
        runtime_id: &str,
        status: &str,
        output: &str,
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await.context("begin job completion")?;
        let owner: Option<String> = sqlx::query_scalar(
            "UPDATE background_jobs SET status = ?, output = ?, finished_at = ?
             WHERE job_id = ? AND owner_runtime_id = ? AND status = 'running' RETURNING thread_id",
        )
        .bind(status)
        .bind(output)
        .bind(timestamp_ms()?)
        .bind(job_id)
        .bind(runtime_id)
        .fetch_optional(&mut *transaction)
        .await
        .context("store job outcome")?;
        if let Some(owner) = owner {
            // Unknown commands may have changed work even when they failed. Refresh
            // their own epoch and invalidate earlier checks before announcing them.
            sqlx::query("UPDATE goals SET work_epoch = work_epoch + 1 WHERE thread_id = ?1
                AND approved_at IS NOT NULL
                AND (revision IS NOT (SELECT goal_revision FROM background_jobs WHERE job_id = ?2)
                OR NOT EXISTS (SELECT 1 FROM json_each(criteria_json) c WHERE
                    json_extract(c.value, '$.check.kind') = 'command' AND json_extract(c.value, '$.check.command') =
                    (SELECT command FROM background_jobs WHERE job_id = ?2)))")
                .bind(&owner).bind(job_id).execute(&mut *transaction).await?;
            // A check keeps its original launch epoch. A pause/resume or another
            // mutation while it ran must not make an old check current again.
            sqlx::query("UPDATE background_jobs SET goal_epoch = (SELECT work_epoch FROM goals WHERE thread_id = ?1)
                WHERE job_id = ?2 AND NOT EXISTS (SELECT 1 FROM goals g, json_each(g.criteria_json) c
                    WHERE g.thread_id = ?1 AND g.revision = background_jobs.goal_revision
                    AND json_extract(c.value, '$.check.kind') = 'command'
                    AND json_extract(c.value, '$.check.command') = background_jobs.command)")
                .bind(&owner).bind(job_id).execute(&mut *transaction).await?;
            let content = format!("[background job {job_id} finished: {status}]\n{output}");
            sqlx::query(
                "INSERT INTO pending_messages (thread_id, request_uid, content, created_at, from_job_id)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(owner).bind(format!("job:{job_id}")).bind(content).bind(timestamp_ms()?).bind(job_id)
            .execute(&mut *transaction).await.context("queue job completion")?;
        }
        transaction.commit().await.context("commit job completion")
    }

    pub async fn unfinished_job_owners(&self) -> Result<Vec<(String, String)>> {
        sqlx::query_as("SELECT DISTINCT owner_runtime_id, owner_lease_path FROM background_jobs
            WHERE status = 'running' AND owner_runtime_id IS NOT NULL AND owner_lease_path IS NOT NULL")
            .fetch_all(&self.pool).await.context("find background job owners")
    }

    /// The caller must hold the owner's abandoned lease through this whole operation.
    /// Its processes may have taken effect. Never start them again.
    pub async fn interrupt_jobs(&self, runtime_id: &str) -> Result<()> {
        let unfinished: Vec<String> = sqlx::query_scalar(
            "SELECT job_id FROM background_jobs WHERE status = 'running' AND owner_runtime_id = ?",
        )
        .bind(runtime_id)
        .fetch_all(&self.pool)
        .await
        .context("find interrupted jobs")?;
        for job_id in unfinished {
            self.finish_job(&job_id, runtime_id, "interrupted",
                "The application stopped before this job's outcome was recorded. Its effects are unknown; it was not restarted.")
                .await?;
        }
        Ok(())
    }

    /// One snapshot closes the race between a foreign job leaving 'running' and
    /// its completion joining pending_messages in the same transaction.
    pub async fn job_wake_state(&self, thread_id: &str, runtime_id: &str) -> Result<(bool, bool)> {
        sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM pending_messages WHERE thread_id = ?),
                EXISTS(SELECT 1 FROM background_jobs WHERE thread_id = ? AND status = 'running'
                    AND (owner_runtime_id IS NULL OR owner_runtime_id != ?))",
        )
        .bind(thread_id)
        .bind(thread_id)
        .bind(runtime_id)
        .fetch_one(&self.pool)
        .await
        .context("check foreign job wake state")
    }

    pub async fn jobs(&self, thread_id: &str) -> Result<Vec<BackgroundJob>> {
        let rows = sqlx::query(
            "SELECT job_id, command, status, output, owner_runtime_id, owner_lease_path FROM background_jobs WHERE thread_id = ? ORDER BY created_at, job_id",
        ).bind(thread_id).fetch_all(&self.pool).await.context("list background jobs")?;
        rows.into_iter()
            .map(|row| {
                Ok(BackgroundJob {
                    job_id: row.try_get("job_id")?,
                    command: row.try_get("command")?,
                    status: row.try_get("status")?,
                    output: row.try_get("output")?,
                    owner_runtime_id: row.try_get("owner_runtime_id")?,
                    owner_lease_path: row.try_get("owner_lease_path")?,
                })
            })
            .collect()
    }

    pub async fn job_summaries(&self, thread_id: &str) -> Result<Vec<JobSummary>> {
        let rows = sqlx::query(
            "SELECT job_id, command, status, created_at, finished_at, owner_runtime_id,
                owner_lease_path FROM background_jobs WHERE thread_id = ?
             ORDER BY created_at, job_id",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .context("list background jobs")?;
        rows.into_iter()
            .map(|row| {
                Ok(JobSummary {
                    job_id: row.try_get("job_id")?,
                    command: row.try_get("command")?,
                    status: row.try_get("status")?,
                    created_at: row.try_get("created_at")?,
                    finished_at: row.try_get("finished_at")?,
                    owner_runtime_id: row.try_get("owner_runtime_id")?,
                    owner_lease_path: row.try_get("owner_lease_path")?,
                })
            })
            .collect()
    }

    pub async fn job(&self, thread_id: &str, job_id: &str) -> Result<BackgroundJob> {
        self.jobs(thread_id)
            .await?
            .into_iter()
            .find(|job| job.job_id == job_id)
            .with_context(|| format!("no background job {job_id} in this thread"))
    }

    pub async fn admit_job(
        &self,
        thread_id: &str,
        job_id: &str,
        content: &str,
    ) -> Result<super::Message> {
        use super::messages::{NewMessage, decode_message, insert_message};
        let mut transaction = self.pool.begin().await.context("begin job admission")?;
        if let Some(row) = sqlx::query("SELECT * FROM messages WHERE from_job_id = ?")
            .bind(job_id)
            .fetch_optional(&mut *transaction)
            .await?
        {
            let message = decode_message(row)?;
            ensure!(
                message.thread_id == thread_id && message.content == content,
                "job completion belongs to a different message"
            );
            return Ok(message);
        }
        let new = NewMessage::job(job_id, content);
        let message =
            insert_message(&mut transaction, thread_id, &new, None, timestamp_ms()?).await?;
        transaction.commit().await.context("commit job admission")?;
        Ok(message)
    }
}
