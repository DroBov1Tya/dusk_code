//! Turns: one per agent run. A turn answers every user message of its thread up to
//! `last_sequence`; user messages after the newest turn are the queue. Inside a turn the
//! agent may take steps: a model call that asks for tools, and what the tools returned.

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};
use uuid::Uuid;

use super::{
    Message, State,
    messages::{NewMessage, insert_message},
    timestamp_ms,
};
use crate::{events::Event, providers::ToolCall};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub turn_id: String,
    pub thread_id: String,
    /// Sequence of the newest user message this turn answers.
    pub last_sequence: u64,
    pub response_message_id: Option<String>,
    pub status: TurnStatus,
    /// Unix timestamps in milliseconds.
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

/// What the agent wrote, stored as an assistant message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnAnswer {
    pub content: String,
    pub reasoning: Option<String>,
}

/// One round of tool use: the agent's message with its calls, and what each call
/// returned, in the same order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnStep {
    pub answer: TurnAnswer,
    pub tool_calls: Vec<ToolCall>,
    pub results: Vec<StepResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepResult {
    pub call_id: String,
    pub content: String,
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
    pub async fn answered_through(&self, thread_id: &str) -> Result<u64> {
        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(last_sequence), 0) FROM turns WHERE thread_id = ?",
        )
        .bind(thread_id)
        .fetch_one(&self.pool)
        .await
        .context("load answered sequence")?;
        sequence.try_into().context("negative turn sequence")
    }

    /// Starts a turn over every queued user message; `None` when nothing is queued.
    /// The database allows one running turn per thread, so a second start fails.
    pub async fn start_turn(&self, thread_id: &str) -> Result<Option<Turn>> {
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
        .bind(Uuid::now_v7().to_string())
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

    /// Stores a step of a running turn in one transaction, so a call is never kept
    /// without its result.
    pub async fn record_step(&self, turn_id: &str, step: TurnStep) -> Result<Vec<Message>> {
        let call_ids = step.tool_calls.iter().map(|call| &call.id);
        ensure!(
            !step.tool_calls.is_empty()
                && call_ids.eq(step.results.iter().map(|result| &result.call_id)),
            "a step needs one result per tool call, in call order"
        );
        let mut new = vec![NewMessage::assistant(
            step.answer.content,
            step.answer.reasoning,
            step.tool_calls,
        )];
        new.extend(
            step.results
                .into_iter()
                .map(|result| NewMessage::tool(result.call_id, result.content)),
        );
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin turn step")?;
        let turn = read_running_turn(&mut transaction, turn_id).await?;
        let mut messages = Vec::with_capacity(new.len());
        for message in &new {
            let stored =
                insert_message(&mut transaction, &turn.thread_id, message, None, timestamp);
            messages.push(stored.await?);
        }
        transaction.commit().await.context("commit turn step")?;
        for message in &messages {
            let _ = self.events.send(Event::MessageCreated(message.clone()));
        }
        Ok(messages)
    }

    /// Records how a running turn ended, together with its answer message.
    pub async fn finish_turn(&self, turn_id: &str, outcome: TurnOutcome) -> Result<Turn> {
        let (status, answer, error) = match outcome {
            TurnOutcome::Completed(answer) => (TurnStatus::Completed, Some(answer), None),
            TurnOutcome::Interrupted(answer) => (TurnStatus::Interrupted, answer, None),
            TurnOutcome::Failed(error) => (TurnStatus::Failed, None, Some(error)),
        };
        let timestamp = timestamp_ms()?;
        let _write = self.writes.lock().await;
        let mut transaction = self.pool.begin().await.context("begin turn finish")?;
        let turn = read_running_turn(&mut transaction, turn_id).await?;
        let message = match answer {
            Some(answer) => {
                ensure!(!answer.content.trim().is_empty(), "answer is empty");
                let new = NewMessage::assistant(answer.content, answer.reasoning, Vec::new());
                let stored =
                    insert_message(&mut transaction, &turn.thread_id, &new, None, timestamp);
                Some(stored.await?)
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
        if let Some(message) = message {
            let _ = self.events.send(Event::MessageCreated(message));
        }
        let _ = self.events.send(Event::TurnFinished(turn.clone()));
        Ok(turn)
    }

    /// A process that stopped mid-turn leaves it running; mark such turns interrupted.
    pub async fn interrupt_running_turns(&self, thread_id: &str) -> Result<()> {
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

async fn read_running_turn(
    transaction: &mut Transaction<'_, Sqlite>,
    turn_id: &str,
) -> Result<Turn> {
    let row = sqlx::query("SELECT * FROM turns WHERE turn_id = ? AND status = 'running'")
        .bind(turn_id)
        .fetch_optional(&mut **transaction)
        .await
        .context("load running turn")?
        .context("turn is not running")?;
    decode_turn(row)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{state::MessageRole, testing::temp_state};

    async fn thread(name: &str) -> State {
        let state = temp_state(name).await;
        state.create_thread("thread", "test").await.unwrap();
        state
    }

    async fn send(state: &State, uid: &str) {
        let text = format!("message {uid}");
        state.add_user_message("thread", uid, &text).await.unwrap();
    }

    fn answer(content: &str, reasoning: Option<&str>) -> TurnAnswer {
        TurnAnswer {
            content: content.into(),
            reasoning: reasoning.map(Into::into),
        }
    }

    #[tokio::test]
    async fn one_turn_answers_every_queued_message() {
        let state = thread("turn_batch").await;
        assert_eq!(state.start_turn("thread").await.unwrap(), None);

        send(&state, "a").await;
        send(&state, "b").await;
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        assert_eq!(turn.last_sequence, 2);
        assert_eq!(
            state.start_turn("thread").await.unwrap(),
            None,
            "nothing new"
        );
        send(&state, "c").await;
        assert!(
            state.start_turn("thread").await.is_err(),
            "one running turn"
        );

        let completed = TurnOutcome::Completed(answer("answer", None));
        let turn = state.finish_turn(&turn.turn_id, completed).await.unwrap();
        assert_eq!(turn.status, TurnStatus::Completed);
        let next = state.start_turn("thread").await.unwrap().unwrap();
        assert_eq!(next.last_sequence, 3, "c was queued during the first turn");
    }

    #[tokio::test]
    async fn a_repeated_request_uid_adds_nothing() {
        let state = thread("turn_repeat").await;
        let first = state
            .add_user_message("thread", "uid", "hello")
            .await
            .unwrap();
        let again = state
            .add_user_message("thread", "uid", "hello")
            .await
            .unwrap();
        assert_eq!(first, again);
        assert!(
            state
                .add_user_message("thread", "uid", "other")
                .await
                .is_err()
        );
        assert!(
            state
                .add_user_message("missing", "uid2", "hi")
                .await
                .is_err()
        );
        assert_eq!(state.list_messages("thread").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_step_keeps_calls_with_their_results() {
        let state = thread("turn_step").await;
        send(&state, "a").await;
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        let call = ToolCall {
            id: "call_1".into(),
            name: "echo".into(),
            arguments: serde_json::json!({}),
        };
        let step = TurnStep {
            answer: answer("", Some("think")),
            tool_calls: vec![call],
            results: vec![StepResult {
                call_id: "call_1".into(),
                content: "pong".into(),
            }],
        };
        state.record_step(&turn.turn_id, step).await.unwrap();
        let wrong = TurnStep {
            answer: answer("", None),
            tool_calls: Vec::new(),
            results: Vec::new(),
        };
        assert!(state.record_step(&turn.turn_id, wrong).await.is_err());

        let messages = state.list_messages("thread").await.unwrap();
        assert_eq!(messages[1].role, MessageRole::Assistant);
        assert_eq!(messages[1].reasoning.as_deref(), Some("think"));
        assert_eq!(messages[1].tool_calls.len(), 1);
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call_1"));
    }

    #[tokio::test]
    async fn interruption_keeps_the_partial_answer() {
        let state = thread("turn_interrupt").await;
        send(&state, "a").await;
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        let partial = TurnOutcome::Interrupted(Some(answer("half an ans", None)));
        let turn = state.finish_turn(&turn.turn_id, partial).await.unwrap();

        assert_eq!(turn.status, TurnStatus::Interrupted);
        let messages = state.list_messages("thread").await.unwrap();
        assert_eq!(messages.last().unwrap().content, "half an ans");
    }

    #[tokio::test]
    async fn a_turn_left_running_is_interrupted_on_recovery() {
        let state = thread("turn_recover").await;
        send(&state, "a").await;
        let turn = state.start_turn("thread").await.unwrap().unwrap();

        state.interrupt_running_turns("thread").await.unwrap();

        let turn = state.load_turn(&turn.turn_id).await.unwrap().unwrap();
        assert_eq!(turn.status, TurnStatus::Interrupted);
        send(&state, "b").await;
        assert!(state.start_turn("thread").await.unwrap().is_some());
    }
}
