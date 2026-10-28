use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use crate::events::{Event, JobCreated, JobUpdated};

use super::{State, timestamp_ms};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobInput {
    pub thread_id: String,
    pub instruction: String,
    pub created_by_agent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub job_id: String,
    pub thread_id: String,
    pub instruction: String,
    pub created_by_agent_id: String,
    pub worker_agent_id: Option<String>,
    /// Unix timestamps in milliseconds. Start/finish describe the current attempt.
    pub created_at: i64,
    pub updated_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    /// Number of explicit requeues; attempt history is not retained here.
    pub retry_count: u64,
    pub status: JobStatus,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum JobOutcome {
    Completed,
    Failed { error_message: String },
    Cancelled,
}

impl State {
    /// Enqueues work under a caller-assigned stable ID. No worker is invoked.
    pub async fn enqueue_job(&self, job_id: &str, input: JobInput) -> Result<Job> {
        ensure!(!job_id.trim().is_empty(), "job ID is empty");
        ensure!(!input.thread_id.trim().is_empty(), "thread ID is empty");
        ensure!(
            !input.instruction.trim().is_empty(),
            "job instruction is empty"
        );
        ensure!(
            !input.created_by_agent_id.trim().is_empty(),
            "creating agent ID is empty"
        );
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin job creation")?;
        let row = sqlx::query(
            "INSERT INTO jobs (job_id, thread_id, instruction, created_by_agent_id,
                created_at, updated_at, status) VALUES (?, ?, ?, ?, ?, ?, 'queued') RETURNING *",
        )
        .bind(job_id)
        .bind(input.thread_id)
        .bind(input.instruction)
        .bind(input.created_by_agent_id)
        .bind(timestamp)
        .bind(timestamp)
        .fetch_one(&mut *transaction)
        .await
        .context("enqueue job")?;
        let job = decode_job(row)?;
        transaction.commit().await.context("commit job creation")?;
        let _ = self
            .events
            .send(Event::JobCreated(JobCreated { job: job.clone() }));
        Ok(job)
    }

    pub async fn load_job(&self, job_id: &str) -> Result<Option<Job>> {
        sqlx::query("SELECT * FROM jobs WHERE job_id = ?")
            .bind(job_id)
            .fetch_optional(&self.pool)
            .await
            .context("load job")?
            .map(decode_job)
            .transpose()
    }

    /// Returns a bounded creation-order view; the scheduler remains a separate concern.
    pub async fn list_jobs(
        &self,
        thread_id: &str,
        status: Option<JobStatus>,
        limit: u32,
    ) -> Result<Vec<Job>> {
        ensure!((1..=1000).contains(&limit), "job limit must be 1..=1000");
        sqlx::query(
            "SELECT * FROM jobs WHERE thread_id = ? AND (? IS NULL OR status = ?)
             ORDER BY created_at, job_id LIMIT ?",
        )
        .bind(thread_id)
        .bind(status.map(JobStatus::as_str))
        .bind(status.map(JobStatus::as_str))
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .context("list jobs")?
        .into_iter()
        .map(decode_job)
        .collect()
    }

    /// Atomically assigns a queued job. Repeating the same active claim is idempotent.
    /// The expected retry count prevents delayed commands from claiming a later attempt.
    pub async fn start_job(
        &self,
        job_id: &str,
        worker_agent_id: &str,
        expected_retry_count: u64,
    ) -> Result<Job> {
        ensure!(
            !worker_agent_id.trim().is_empty(),
            "worker agent ID is empty"
        );
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin job start")?;
        let row = sqlx::query(
            "UPDATE jobs SET worker_agent_id = ?, started_at = MAX(updated_at, ?),
                updated_at = MAX(updated_at, ?), status = 'running'
             WHERE job_id = ? AND status = 'queued' AND retry_count = ? RETURNING *",
        )
        .bind(worker_agent_id)
        .bind(timestamp)
        .bind(timestamp)
        .bind(job_id)
        .bind(retry_count_sql(expected_retry_count)?)
        .fetch_optional(&mut *transaction)
        .await
        .context("start job")?;
        let changed = row.is_some();
        let row = match row {
            Some(row) => row,
            None => sqlx::query("SELECT * FROM jobs WHERE job_id = ?")
                .bind(job_id)
                .fetch_optional(&mut *transaction)
                .await?
                .context("job does not exist")?,
        };
        let job = decode_job(row)?;
        ensure!(
            job.retry_count == expected_retry_count
                && job.status == JobStatus::Running
                && job.worker_agent_id.as_deref() == Some(worker_agent_id),
            "job is not available to this worker"
        );
        transaction.commit().await.context("commit job start")?;
        if changed {
            let _ = self
                .events
                .send(Event::JobUpdated(JobUpdated { job: job.clone() }));
        }
        Ok(job)
    }

    /// Only the assigned worker may finish a running job. Repeating its same result is idempotent.
    /// The expected retry count also fences results from an earlier attempt by the same worker.
    pub async fn finish_job(
        &self,
        job_id: &str,
        worker_agent_id: &str,
        expected_retry_count: u64,
        outcome: JobOutcome,
    ) -> Result<Job> {
        ensure!(
            !worker_agent_id.trim().is_empty(),
            "worker agent ID is empty"
        );
        let (status, error_message) = match outcome {
            JobOutcome::Completed => (JobStatus::Completed, None),
            JobOutcome::Cancelled => (JobStatus::Cancelled, None),
            JobOutcome::Failed { error_message } => {
                ensure!(
                    !error_message.trim().is_empty(),
                    "job error message is empty"
                );
                (JobStatus::Failed, Some(error_message))
            }
        };
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin job finish")?;
        let row = sqlx::query(
            "UPDATE jobs SET status = ?, error_message = ?, finished_at = MAX(updated_at, ?),
                updated_at = MAX(updated_at, ?)
             WHERE job_id = ? AND status = 'running' AND worker_agent_id = ?
                AND retry_count = ? RETURNING *",
        )
        .bind(status.as_str())
        .bind(&error_message)
        .bind(timestamp)
        .bind(timestamp)
        .bind(job_id)
        .bind(worker_agent_id)
        .bind(retry_count_sql(expected_retry_count)?)
        .fetch_optional(&mut *transaction)
        .await
        .context("finish job")?;
        let changed = row.is_some();
        let row = match row {
            Some(row) => row,
            None => sqlx::query("SELECT * FROM jobs WHERE job_id = ?")
                .bind(job_id)
                .fetch_optional(&mut *transaction)
                .await?
                .context("job does not exist")?,
        };
        let job = decode_job(row)?;
        ensure!(
            job.retry_count == expected_retry_count
                && job.status == status
                && job.worker_agent_id.as_deref() == Some(worker_agent_id)
                && job.error_message == error_message,
            "job cannot be finished by this worker with this result"
        );
        transaction.commit().await.context("commit job finish")?;
        if changed {
            let _ = self
                .events
                .send(Event::JobUpdated(JobUpdated { job: job.clone() }));
        }
        Ok(job)
    }

    /// Cancels work that has not been assigned. Running workers use `finish_job`.
    pub async fn cancel_queued_job(&self, job_id: &str, expected_retry_count: u64) -> Result<Job> {
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self
            .pool
            .begin()
            .await
            .context("begin queued job cancellation")?;
        let row = sqlx::query(
            "UPDATE jobs SET status = 'cancelled', finished_at = MAX(updated_at, ?),
                updated_at = MAX(updated_at, ?)
             WHERE job_id = ? AND status = 'queued' AND retry_count = ? RETURNING *",
        )
        .bind(timestamp)
        .bind(timestamp)
        .bind(job_id)
        .bind(retry_count_sql(expected_retry_count)?)
        .fetch_optional(&mut *transaction)
        .await
        .context("cancel queued job")?
        .context("job is not queued")?;
        let job = decode_job(row)?;
        transaction
            .commit()
            .await
            .context("commit queued job cancellation")?;
        let _ = self
            .events
            .send(Event::JobUpdated(JobUpdated { job: job.clone() }));
        Ok(job)
    }

    /// Requeues a failed/cancelled job. Assignment, error and current-attempt times are reset.
    pub async fn retry_job(&self, job_id: &str, expected_retry_count: u64) -> Result<Job> {
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin job retry")?;
        let row = sqlx::query(
            "UPDATE jobs SET status = 'queued', worker_agent_id = NULL, started_at = NULL,
                finished_at = NULL, error_message = NULL, retry_count = retry_count + 1,
                updated_at = MAX(updated_at, ?)
             WHERE job_id = ? AND status IN ('failed', 'cancelled') AND retry_count = ?
                AND retry_count < 9223372036854775807 RETURNING *",
        )
        .bind(timestamp)
        .bind(job_id)
        .bind(retry_count_sql(expected_retry_count)?)
        .fetch_optional(&mut *transaction)
        .await
        .context("retry job")?
        .context("job is not retryable")?;
        let job = decode_job(row)?;
        transaction.commit().await.context("commit job retry")?;
        let _ = self
            .events
            .send(Event::JobUpdated(JobUpdated { job: job.clone() }));
        Ok(job)
    }
}

fn retry_count_sql(retry_count: u64) -> Result<i64> {
    i64::try_from(retry_count).context("retry count exceeds SQLite integer range")
}

fn decode_job(row: SqliteRow) -> Result<Job> {
    let status: String = row.try_get("status")?;
    let status = match status.as_str() {
        "queued" => JobStatus::Queued,
        "running" => JobStatus::Running,
        "completed" => JobStatus::Completed,
        "failed" => JobStatus::Failed,
        "cancelled" => JobStatus::Cancelled,
        _ => bail!("invalid stored job status"),
    };
    let retry_count: i64 = row.try_get("retry_count")?;
    Ok(Job {
        job_id: row.try_get("job_id")?,
        thread_id: row.try_get("thread_id")?,
        instruction: row.try_get("instruction")?,
        created_by_agent_id: row.try_get("created_by_agent_id")?,
        worker_agent_id: row.try_get("worker_agent_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        retry_count: retry_count.try_into().context("negative retry count")?,
        status,
        error_message: row.try_get("error_message")?,
    })
}
