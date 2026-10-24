use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use crate::events::{ExecutionFailed, MessageCreated, TaskAccepted, TaskRejected, TurnCreated};
use crate::{Event, Execution, ExecutionFailureCode, ExecutionStatus, RejectionCode};

use super::{
    MessageInput, MessageRole, State,
    messages::{decode_message, insert_message, validate_context},
    timestamp_ms,
    turns::insert_turn,
};

impl State {
    /// Commits a receipt and accepted message atomically. The receipt remains metadata-only.
    pub(crate) async fn receive_request(
        &self,
        request_uid: &str,
        input: MessageInput,
        rejection: Option<RejectionCode>,
    ) -> Result<Execution> {
        ensure!(!request_uid.trim().is_empty(), "request UID is empty");
        ensure!(
            input.role == MessageRole::User && input.job_id.is_none() && input.agent_id.is_none(),
            "invalid user-message input"
        );
        validate_context(&input.cwd, &input.attachments)?;
        let thread_id = &input.thread_id;
        let received_at = timestamp_ms()?;
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
            &execution.thread_id == thread_id,
            "request UID already belongs to another thread"
        );
        let mut original_message = None;
        let message = if inserted && execution.status == ExecutionStatus::Accepted {
            // User request IDs form a stable message ID namespace for transport replays.
            Some(
                insert_message(
                    &mut transaction,
                    &format!("request:{request_uid}"),
                    &input,
                    Some(request_uid),
                    received_at,
                )
                .await?,
            )
        } else {
            if !inserted && execution.status != ExecutionStatus::Rejected {
                let original = sqlx::query("SELECT * FROM messages WHERE request_uid = ?")
                    .bind(request_uid)
                    .fetch_optional(&mut *transaction)
                    .await
                    .context("load original request message")?
                    .context("existing request has no persisted message")?;
                let original = decode_message(original)?;
                ensure!(
                    original.thread_id == input.thread_id
                        && original.content == input.content
                        && original.cwd == input.cwd
                        && original.attachments == input.attachments,
                    "request UID already belongs to different input"
                );
                original_message = Some(original);
            }
            // Rejected receipts have no persisted payload to compare; return the original receipt.
            None
        };
        let turn = if execution.status == ExecutionStatus::Accepted {
            let user_message = message
                .as_ref()
                .or(original_message.as_ref())
                .context("accepted request has no message")?;
            insert_turn(
                &mut transaction,
                request_uid,
                user_message,
                execution.received_at,
            )
            .await?
        } else {
            None
        };
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
                ExecutionStatus::Failed => Event::ExecutionFailed(ExecutionFailed {
                    execution: execution.clone(),
                }),
            };
            let _ = self.events.send(event);
            if let Some(message) = message {
                let _ = self
                    .events
                    .send(Event::MessageCreated(MessageCreated { message }));
            }
        }
        if let Some(turn) = turn {
            let _ = self.events.send(Event::TurnCreated(TurnCreated { turn }));
        }
        Ok(execution)
    }

    /// Legacy failure path for receipts without a user turn, without persisting error text.
    /// User-turn results must go through AgentReceiver with agent and attempt validation.
    /// Repeating a failed UID returns the original result without another event.
    pub async fn fail_execution(&self, request_uid: &str) -> Result<Execution> {
        ensure!(!request_uid.trim().is_empty(), "request UID is empty");
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin execution failure")?;
        let has_turn: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM turns WHERE turn_id = ?)")
                .bind(request_uid)
                .fetch_one(&mut *transaction)
                .await
                .context("inspect execution turn")?;
        ensure!(
            !has_turn,
            "user turn lifecycle must be changed through the agent receiver"
        );
        let row = sqlx::query(
            "UPDATE event_logs SET status = 'failed', error_code = ?
             WHERE request_uid = ? AND status = 'accepted' RETURNING *",
        )
        .bind(ExecutionFailureCode::ExecutionFailed.as_str())
        .bind(request_uid)
        .fetch_optional(&mut *transaction)
        .await
        .context("record execution failure")?;
        let changed = row.is_some();
        let row = match row {
            Some(row) => row,
            None => sqlx::query("SELECT * FROM event_logs WHERE request_uid = ?")
                .bind(request_uid)
                .fetch_optional(&mut *transaction)
                .await
                .context("load execution for failure")?
                .context("execution does not exist")?,
        };
        let execution = decode_execution(row)?;
        ensure!(
            execution.status == ExecutionStatus::Failed,
            "only accepted executions can fail"
        );
        transaction
            .commit()
            .await
            .context("commit execution failure")?;
        if changed {
            let _ = self.events.send(Event::ExecutionFailed(ExecutionFailed {
                execution: execution.clone(),
            }));
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
        "failed" => ExecutionStatus::Failed,
        _ => bail!("invalid stored execution status"),
    };
    let error_code: Option<String> = row.try_get("error_code")?;
    let (error_code, failure_code) = match (status, error_code.as_deref()) {
        (ExecutionStatus::Accepted, None) => (None, None),
        (ExecutionStatus::Rejected, Some("empty_message")) => {
            (Some(RejectionCode::EmptyMessage), None)
        }
        (ExecutionStatus::Rejected, Some("empty_thread")) => {
            (Some(RejectionCode::EmptyThread), None)
        }
        (ExecutionStatus::Rejected, Some("unknown_thread")) => {
            (Some(RejectionCode::UnknownThread), None)
        }
        (ExecutionStatus::Failed, Some("execution_failed")) => {
            (None, Some(ExecutionFailureCode::ExecutionFailed))
        }
        _ => bail!("stored execution status and error code disagree"),
    };
    Ok(Execution {
        request_uid: row.try_get("request_uid")?,
        thread_id: row.try_get("thread_id")?,
        received_at: row.try_get("received_at")?,
        status,
        error_code,
        failure_code,
    })
}
