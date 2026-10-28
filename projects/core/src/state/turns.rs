//! Turns: one per agent run. A turn answers every user message of its thread up to
//! `last_sequence`; user messages after the newest turn are the queue.

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

use crate::events::Event;

use super::{Message, MessageInput, MessageRole, State, messages::insert_message, timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub turn_id: String,
    pub thread_id: String,
    /// Sequence of the newest user message this turn answers.
    pub last_sequence: u64,
    pub response_message_id: Option<String>,
    pub status: TurnStatus,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TurnStatus {
    Running,
    Completed,
    /// Stopped by the user or a restart; a partial answer is kept when there was one.
    Interrupted,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TurnOutcome {
    Completed(TurnAnswer),
    Interrupted(Option<TurnAnswer>),
    Failed(String),
}

/// What the agent wrote; stored as the turn's assistant message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnAnswer {
    pub content: String,
    pub reasoning: Option<String>,
    pub agent_id: String,
}

impl TurnStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }
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

    /// Sequence of the newest message any turn of the thread has taken; 0 before the first.
    pub(crate) async fn answered_through(&self, thread_id: &str) -> Result<u64> {
        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(last_sequence), 0) FROM turns WHERE thread_id = ?",
        )
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await
        .context("load answered sequence")?;
        sequence.try_into().context("negative turn sequence")
    }

    /// Starts a turn over every queued user message. Returns `None` when nothing is queued.
    /// The database allows one running turn per thread, so a second start fails.
    pub(crate) async fn start_turn(&self, thread_id: &str, turn_id: &str) -> Result<Option<Turn>> {
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let row = sqlx::query(
            "INSERT INTO turns (turn_id, thread_id, last_sequence, status, started_at)
             SELECT ?, ?, MAX(sequence), 'running', ? FROM messages
             WHERE thread_id = ? AND role = 'user'
               AND sequence > (SELECT COALESCE(MAX(last_sequence), 0) FROM turns WHERE thread_id = ?)
             HAVING COUNT(*) > 0
             RETURNING *",
        )
        .bind(turn_id)
        .bind(thread_id)
        .bind(timestamp)
        .bind(thread_id)
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await
        .context("start turn")?;
        let Some(row) = row else {
            return Ok(None);
        };
        let turn = decode_turn(row)?;
        let _ = self.events.send(Event::TurnStarted(turn.clone()));
        Ok(Some(turn))
    }

    /// Records the outcome of a running turn together with its answer message.
    pub(crate) async fn finish_turn(
        &self,
        turn_id: &str,
        outcome: TurnOutcome,
    ) -> Result<(Turn, Option<Message>)> {
        let (status, answer, error) = match outcome {
            TurnOutcome::Completed(answer) => (TurnStatus::Completed, Some(answer), None),
            TurnOutcome::Interrupted(answer) => (TurnStatus::Interrupted, answer, None),
            TurnOutcome::Failed(error) => (TurnStatus::Failed, None, Some(error)),
        };
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin turn finish")?;
        let turn = sqlx::query("SELECT * FROM turns WHERE turn_id = ? AND status = 'running'")
            .bind(turn_id)
            .fetch_optional(&mut *transaction)
            .await
            .context("load running turn")?
            .context("turn is not running")?;
        let turn = decode_turn(turn)?;
        let message = match answer {
            Some(answer) => {
                let input = answer_message(&mut transaction, &turn, answer).await?;
                let message_id = format!("turn-response:{turn_id}");
                Some(insert_message(&mut transaction, &message_id, &input, None, timestamp).await?)
            }
            None => None,
        };
        let row = sqlx::query(
            "UPDATE turns SET status = ?, response_message_id = ?, error_message = ?,
                finished_at = MAX(started_at, ?)
             WHERE turn_id = ? RETURNING *",
        )
        .bind(status.as_str())
        .bind(message.as_ref().map(|message| &message.message_id))
        .bind(error)
        .bind(timestamp)
        .bind(turn_id)
        .fetch_one(&mut *transaction)
        .await
        .context("finish turn")?;
        let turn = decode_turn(row)?;
        transaction.commit().await.context("commit turn finish")?;
        if let Some(message) = &message {
            let _ = self.events.send(Event::MessageCreated(message.clone()));
        }
        let _ = self.events.send(Event::TurnFinished(turn.clone()));
        Ok((turn, message))
    }

    /// A process that stopped mid-turn leaves it running; mark such turns interrupted.
    pub(crate) async fn interrupt_running_turns(&self, thread_id: &str) -> Result<()> {
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        sqlx::query(
            "UPDATE turns SET status = 'interrupted', finished_at = MAX(started_at, ?)
             WHERE thread_id = ? AND status = 'running'",
        )
        .bind(timestamp)
        .bind(thread_id)
        .execute(&self.pool)
        .await
        .context("interrupt running turns")?;
        Ok(())
    }
}

/// The answer inherits the working directory of the newest message it answers.
async fn answer_message(
    transaction: &mut Transaction<'_, Sqlite>,
    turn: &Turn,
    answer: TurnAnswer,
) -> Result<MessageInput> {
    ensure!(!answer.content.trim().is_empty(), "answer is empty");
    let cwd: String =
        sqlx::query_scalar("SELECT cwd FROM messages WHERE thread_id = ? AND sequence = ?")
            .bind(&turn.thread_id)
            .bind(i64::try_from(turn.last_sequence).context("sequence out of range")?)
            .fetch_one(&mut **transaction)
            .await
            .context("load answered message")?;
    Ok(MessageInput {
        thread_id: turn.thread_id.clone(),
        job_id: None,
        role: MessageRole::Assistant,
        agent_id: Some(answer.agent_id),
        content: answer.content,
        reasoning: answer.reasoning,
        cwd,
        attachments: Vec::new(),
    })
}

fn decode_turn(row: SqliteRow) -> Result<Turn> {
    let status: String = row.try_get("status")?;
    let status = match status.as_str() {
        "running" => TurnStatus::Running,
        "completed" => TurnStatus::Completed,
        "interrupted" => TurnStatus::Interrupted,
        "failed" => TurnStatus::Failed,
        _ => bail!("invalid stored turn status"),
    };
    let last_sequence: i64 = row.try_get("last_sequence")?;
    Ok(Turn {
        turn_id: row.try_get("turn_id")?,
        thread_id: row.try_get("thread_id")?,
        last_sequence: last_sequence.try_into().context("negative turn sequence")?,
        response_message_id: row.try_get("response_message_id")?,
        status,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        error_message: row.try_get("error_message")?,
    })
}
