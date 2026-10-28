use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

use crate::events::{Event, MessageCreated, TurnUpdated};

use super::{
    Message, MessageAttachment, MessageInput, MessageRole, State,
    messages::{decode_message, insert_message},
    timestamp_ms,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// The stable request UID is also the user turn ID.
    pub turn_id: String,
    pub thread_id: String,
    pub user_message_id: String,
    pub response_message_id: Option<String>,
    pub agent_id: Option<String>,
    /// Fences internal responses; retries are not exposed by the receiver yet.
    pub attempt: u64,
    pub status: TurnStatus,
    pub created_at: i64,
    pub updated_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub error_message: Option<String>,
}

impl State {
    pub async fn load_turn(&self, turn_id: &str) -> Result<Option<Turn>> {
        sqlx::query("SELECT * FROM turns WHERE turn_id = ?")
            .bind(turn_id)
            .fetch_optional(&self.pool)
            .await
            .context("load turn")?
            .map(decode_turn)
            .transpose()
    }

    pub(crate) async fn pending_turns(&self, thread_id: &str, limit: u32) -> Result<Vec<Turn>> {
        ensure!((1..=1000).contains(&limit), "turn limit must be 1..=1000");
        sqlx::query(
            "SELECT turns.* FROM turns
             JOIN messages ON messages.message_id = turns.user_message_id
             WHERE turns.thread_id = ? AND turns.status = 'queued'
             ORDER BY messages.sequence LIMIT ?",
        )
        .bind(thread_id)
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await
        .context("load pending turns")?
        .into_iter()
        .map(decode_turn)
        .collect()
    }

    pub(crate) async fn start_turn(
        &self,
        turn_id: &str,
        agent_id: &str,
        expected_attempt: u64,
    ) -> Result<Turn> {
        let timestamp = timestamp_ms()?;
        let attempt = attempt_sql(expected_attempt)?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin turn start")?;
        let row = sqlx::query(
            "UPDATE turns SET status = 'running', agent_id = ?,
                started_at = MAX(updated_at, ?), updated_at = MAX(updated_at, ?)
             WHERE turn_id = ? AND status = 'queued' AND attempt = ? RETURNING *",
        )
        .bind(agent_id)
        .bind(timestamp)
        .bind(timestamp)
        .bind(turn_id)
        .bind(attempt)
        .fetch_optional(&mut *transaction)
        .await
        .context("start turn")?;
        let changed = row.is_some();
        let turn = match row {
            Some(row) => decode_turn(row)?,
            None => read_turn(&mut transaction, turn_id).await?,
        };
        ensure!(
            turn.status == TurnStatus::Running
                && turn.agent_id.as_deref() == Some(agent_id)
                && turn.attempt == expected_attempt,
            "turn is not available to this agent and attempt"
        );
        transaction.commit().await.context("commit turn start")?;
        if changed {
            let _ = self
                .events
                .send(Event::TurnUpdated(TurnUpdated { turn: turn.clone() }));
        }
        Ok(turn)
    }

    pub(crate) async fn complete_turn(
        &self,
        turn_id: &str,
        agent_id: &str,
        expected_attempt: u64,
        content: String,
        reasoning: Option<String>,
        attachments: Vec<MessageAttachment>,
    ) -> Result<(Turn, Message)> {
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin turn response")?;
        let turn = read_turn(&mut transaction, turn_id).await?;
        ensure!(
            turn.agent_id.as_deref() == Some(agent_id) && turn.attempt == expected_attempt,
            "turn does not belong to this agent and attempt"
        );
        if turn.status == TurnStatus::Completed {
            let message_id = turn
                .response_message_id
                .as_deref()
                .context("turn has no response")?;
            let message = sqlx::query("SELECT * FROM messages WHERE message_id = ?")
                .bind(message_id)
                .fetch_one(&mut *transaction)
                .await
                .context("load turn response")?;
            let message = decode_message(message)?;
            ensure!(
                message.content == content
                    && message.reasoning == reasoning
                    && message.attachments == attachments,
                "turn already has a different response"
            );
            transaction
                .commit()
                .await
                .context("commit response replay")?;
            return Ok((turn, message));
        }
        ensure!(
            turn.status == TurnStatus::Running,
            "only running turns accept a response"
        );
        let cwd: String = sqlx::query_scalar("SELECT cwd FROM messages WHERE message_id = ?")
            .bind(&turn.user_message_id)
            .fetch_one(&mut *transaction)
            .await
            .context("load turn context")?;
        let message_id = format!(
            "turn-response:{}:{turn_id}:{expected_attempt}",
            turn_id.len()
        );
        let message = insert_message(
            &mut transaction,
            &message_id,
            &MessageInput {
                thread_id: turn.thread_id.clone(),
                job_id: None,
                role: MessageRole::Assistant,
                agent_id: Some(agent_id.to_owned()),
                content,
                reasoning,
                cwd,
                attachments,
            },
            None,
            timestamp,
        )
        .await?;
        let row = sqlx::query(
            "UPDATE turns SET status = 'completed', response_message_id = ?,
                updated_at = MAX(updated_at, ?), finished_at = MAX(updated_at, ?)
             WHERE turn_id = ? AND status = 'running' AND agent_id = ? AND attempt = ? RETURNING *",
        )
        .bind(&message.message_id)
        .bind(timestamp)
        .bind(timestamp)
        .bind(turn_id)
        .bind(agent_id)
        .bind(attempt_sql(expected_attempt)?)
        .fetch_optional(&mut *transaction)
        .await
        .context("complete turn")?
        .context("turn changed before completion")?;
        let turn = decode_turn(row)?;
        transaction.commit().await.context("commit turn response")?;
        let _ = self.events.send(Event::MessageCreated(MessageCreated {
            message: message.clone(),
        }));
        let _ = self
            .events
            .send(Event::TurnUpdated(TurnUpdated { turn: turn.clone() }));
        Ok((turn, message))
    }

    pub(crate) async fn fail_turn(
        &self,
        turn_id: &str,
        agent_id: &str,
        expected_attempt: u64,
        error_message: String,
    ) -> Result<Turn> {
        ensure!(!error_message.trim().is_empty(), "turn error is empty");
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin turn failure")?;
        let row = sqlx::query(
            "UPDATE turns SET status = 'failed', error_message = ?,
                updated_at = MAX(updated_at, ?), finished_at = MAX(updated_at, ?)
             WHERE turn_id = ? AND status = 'running' AND agent_id = ? AND attempt = ? RETURNING *",
        )
        .bind(&error_message)
        .bind(timestamp)
        .bind(timestamp)
        .bind(turn_id)
        .bind(agent_id)
        .bind(attempt_sql(expected_attempt)?)
        .fetch_optional(&mut *transaction)
        .await
        .context("fail turn")?;
        let changed = row.is_some();
        let turn = match row {
            Some(row) => decode_turn(row)?,
            None => read_turn(&mut transaction, turn_id).await?,
        };
        ensure!(
            turn.status == TurnStatus::Failed
                && turn.agent_id.as_deref() == Some(agent_id)
                && turn.attempt == expected_attempt
                && turn.error_message.as_deref() == Some(&error_message),
            "turn cannot fail with this agent, attempt and result"
        );
        transaction.commit().await.context("commit turn failure")?;
        if changed {
            let _ = self
                .events
                .send(Event::TurnUpdated(TurnUpdated { turn: turn.clone() }));
        }
        Ok(turn)
    }
}

pub(super) async fn insert_turn(
    transaction: &mut Transaction<'_, Sqlite>,
    request_uid: &str,
    message: &Message,
    timestamp: i64,
) -> Result<Option<Turn>> {
    sqlx::query(
        "INSERT INTO turns (turn_id, thread_id, user_message_id, status, created_at, updated_at)
         VALUES (?, ?, ?, 'queued', ?, ?) ON CONFLICT(turn_id) DO NOTHING RETURNING *",
    )
    .bind(request_uid)
    .bind(&message.thread_id)
    .bind(&message.message_id)
    .bind(timestamp)
    .bind(timestamp)
    .fetch_optional(&mut **transaction)
    .await
    .context("create user turn")?
    .map(decode_turn)
    .transpose()
}

async fn read_turn(transaction: &mut Transaction<'_, Sqlite>, turn_id: &str) -> Result<Turn> {
    let row = sqlx::query("SELECT * FROM turns WHERE turn_id = ?")
        .bind(turn_id)
        .fetch_optional(&mut **transaction)
        .await
        .context("load active turn")?
        .context("turn does not exist")?;
    decode_turn(row)
}

fn attempt_sql(attempt: u64) -> Result<i64> {
    i64::try_from(attempt).context("turn attempt exceeds SQLite integer range")
}

fn decode_turn(row: SqliteRow) -> Result<Turn> {
    let status: String = row.try_get("status")?;
    let status = match status.as_str() {
        "queued" => TurnStatus::Queued,
        "running" => TurnStatus::Running,
        "completed" => TurnStatus::Completed,
        "failed" => TurnStatus::Failed,
        _ => bail!("invalid stored turn status"),
    };
    let attempt: i64 = row.try_get("attempt")?;
    Ok(Turn {
        turn_id: row.try_get("turn_id")?,
        thread_id: row.try_get("thread_id")?,
        user_message_id: row.try_get("user_message_id")?,
        response_message_id: row.try_get("response_message_id")?,
        agent_id: row.try_get("agent_id")?,
        attempt: attempt.try_into().context("negative turn attempt")?,
        status,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        error_message: row.try_get("error_message")?,
    })
}
