//! Request receipts: one durable, content-free record per user request UID.

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, sqlite::SqliteRow};

use crate::events::Event;

use super::{
    Message, MessageInput, MessageRole, State,
    messages::{decode_message, insert_message, validate_context},
    timestamp_ms,
};

/// Receipt of a user request, without its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub request_uid: String,
    pub thread_id: String,
    /// Unix timestamp in milliseconds, assigned on first receipt.
    pub received_at: i64,
    pub status: ExecutionStatus,
    /// Set exactly when the request was rejected.
    pub error_code: Option<RejectionCode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExecutionStatus {
    /// The message is stored and queued for the agent.
    Accepted,
    Rejected,
}

/// Fixed validation codes never contain user-message content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RejectionCode {
    EmptyMessage,
    EmptyThread,
    UnknownThread,
}

impl ExecutionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }
}

impl RejectionCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyMessage => "empty_message",
            Self::EmptyThread => "empty_thread",
            Self::UnknownThread => "unknown_thread",
        }
    }
}

impl State {
    /// Commits the receipt and, when accepted, the message in one transaction. A repeated
    /// UID returns the original receipt and message without new events; it must carry the
    /// same input. Unknown threads are rejected by the same statement that records the receipt.
    pub(crate) async fn receive_request(
        &self,
        request_uid: &str,
        input: MessageInput,
        rejection: Option<RejectionCode>,
    ) -> Result<(Execution, Option<Message>)> {
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
        let message = match (execution.status, inserted) {
            (ExecutionStatus::Rejected, _) => None,
            (ExecutionStatus::Accepted, true) => {
                // Request UIDs form a stable message ID namespace for replays.
                let message_id = format!("request:{request_uid}");
                let message = insert_message(
                    &mut transaction,
                    &message_id,
                    &input,
                    Some(request_uid),
                    received_at,
                )
                .await?;
                Some(message)
            }
            (ExecutionStatus::Accepted, false) => {
                let original = sqlx::query("SELECT * FROM messages WHERE request_uid = ?")
                    .bind(request_uid)
                    .fetch_one(&mut *transaction)
                    .await
                    .context("load original request message")?;
                let original = decode_message(original)?;
                ensure!(
                    original.content == input.content
                        && original.cwd == input.cwd
                        && original.attachments == input.attachments,
                    "request UID already belongs to different input"
                );
                Some(original)
            }
        };
        transaction
            .commit()
            .await
            .context("commit request receipt")?;
        if inserted {
            let event = match execution.status {
                ExecutionStatus::Accepted => Event::TaskAccepted(execution.clone()),
                ExecutionStatus::Rejected => Event::TaskRejected(execution.clone()),
            };
            let _ = self.events.send(event);
            if let Some(message) = &message {
                let _ = self.events.send(Event::MessageCreated(message.clone()));
            }
        }
        Ok((execution, message))
    }

    /// Missing requests return `None`.
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
    let error_code: Option<String> = row.try_get("error_code")?;
    let (status, error_code) = match (status.as_str(), error_code.as_deref()) {
        ("accepted", None) => (ExecutionStatus::Accepted, None),
        ("rejected", Some("empty_message")) => {
            (ExecutionStatus::Rejected, Some(RejectionCode::EmptyMessage))
        }
        ("rejected", Some("empty_thread")) => {
            (ExecutionStatus::Rejected, Some(RejectionCode::EmptyThread))
        }
        ("rejected", Some("unknown_thread")) => (
            ExecutionStatus::Rejected,
            Some(RejectionCode::UnknownThread),
        ),
        _ => bail!("invalid stored receipt status or code"),
    };
    Ok(Execution {
        request_uid: row.try_get("request_uid")?,
        thread_id: row.try_get("thread_id")?,
        received_at: row.try_get("received_at")?,
        status,
        error_code,
    })
}
