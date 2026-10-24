use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use crate::events::{TaskAccepted, TaskRejected};
use crate::{Event, Execution, ExecutionStatus, RejectionCode};

use super::State;

impl State {
    /// Only the transport can receive input; storage never receives message content.
    pub(crate) async fn receive_request(
        &self,
        request_uid: &str,
        thread_id: &str,
        rejection: Option<RejectionCode>,
    ) -> Result<Execution> {
        ensure!(!request_uid.trim().is_empty(), "request UID is empty");
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system clock precedes Unix epoch")?;
        let received_at =
            i64::try_from(elapsed.as_millis()).context("timestamp exceeds SQLite integer range")?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin request receipt")?;
        let row = sqlx::query(
            "INSERT INTO event_logs (request_uid, thread_id, received_at, status, error_code)
             VALUES (?, ?, ?,
                CASE WHEN ? IS NOT NULL THEN 'rejected'
                     WHEN EXISTS (SELECT 1 FROM threads WHERE id = ?) THEN 'accepted'
                     ELSE 'rejected' END,
                COALESCE(?, CASE WHEN EXISTS (SELECT 1 FROM threads WHERE id = ?) THEN NULL
                                ELSE 'unknown_thread' END))
             ON CONFLICT(request_uid) DO NOTHING RETURNING *",
        )
        .bind(request_uid)
        .bind(thread_id)
        .bind(received_at)
        .bind(rejection.map(RejectionCode::as_str))
        .bind(thread_id)
        .bind(rejection.map(RejectionCode::as_str))
        .bind(thread_id)
        .fetch_optional(&mut *transaction)
        .await
        .context("record request metadata")?;
        let inserted = row.is_some();
        let row = match row {
            Some(row) => row,
            None => sqlx::query("SELECT * FROM event_logs WHERE request_uid = ?")
                .bind(request_uid)
                .fetch_one(&mut *transaction)
                .await
                .context("load existing request receipt")?,
        };
        let execution = decode_execution(row)?;
        ensure!(
            execution.thread_id == thread_id,
            "request UID already belongs to another thread"
        );
        transaction
            .commit()
            .await
            .context("commit request receipt")?;
        if inserted {
            let event = match execution.status {
                ExecutionStatus::Accepted => Event::TaskAccepted(TaskAccepted {
                    execution: execution.clone(),
                }),
                ExecutionStatus::Rejected => Event::TaskRejected(TaskRejected {
                    execution: execution.clone(),
                }),
            };
            let _ = self.events.send(event);
        }
        Ok(execution)
    }

    /// Reads a receipt without message content. Missing requests return `None`.
    pub async fn load_execution(&self, request_uid: &str) -> Result<Option<Execution>> {
        sqlx::query("SELECT * FROM event_logs WHERE request_uid = ?")
            .bind(request_uid)
            .fetch_optional(&self.pool)
            .await
            .context("load request receipt")?
            .map(decode_execution)
            .transpose()
    }
}

fn decode_execution(row: SqliteRow) -> Result<Execution> {
    let status: String = row.try_get("status")?;
    let status = match status.as_str() {
        "accepted" => ExecutionStatus::Accepted,
        "rejected" => ExecutionStatus::Rejected,
        _ => bail!("invalid stored execution status"),
    };
    let error_code: Option<String> = row.try_get("error_code")?;
    let error_code = match error_code.as_deref() {
        Some("empty_message") => Some(RejectionCode::EmptyMessage),
        Some("empty_thread") => Some(RejectionCode::EmptyThread),
        Some("unknown_thread") => Some(RejectionCode::UnknownThread),
        Some(_) => bail!("invalid stored rejection code"),
        None => None,
    };
    ensure!(
        (status == ExecutionStatus::Rejected) == error_code.is_some(),
        "stored execution status and rejection code disagree"
    );
    Ok(Execution {
        request_uid: row.try_get("request_uid")?,
        thread_id: row.try_get("thread_id")?,
        received_at: row.try_get("received_at")?,
        status,
        error_code,
    })
}
