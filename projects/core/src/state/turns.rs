//! Turns: one per agent run. A turn answers every user message of its thread up to
//! `last_sequence`; user messages after the newest turn are the queue. Inside a turn the
//! agent may take steps: a model call that asks for tools, and what the tools returned.
//!
//! A step is written ahead: the calls before any tool runs, each result right after its
//! tool. A call left without a result, by a crash or a failed write, gets a placeholder
//! when its turn ends, because a model needs one result per call.

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};
use uuid::Uuid;

use super::{
    Message, State,
    messages::{NewMessage, insert_message},
    timestamp_ms,
};
use crate::providers::ToolCall;

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

impl State {
    pub async fn count_turns(&self, thread_id: &str) -> Result<u64> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM turns WHERE thread_id = ?")
            .bind(thread_id)
            .fetch_one(&self.pool)
            .await
            .context("count turns")?;
        count.try_into().context("negative turn count")
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
        Ok(Some(turn))
    }

    /// Stores the agent's message with its tool calls, before any of them runs.
    pub async fn record_tool_calls(
        &self,
        turn_id: &str,
        answer: TurnAnswer,
        calls: Vec<ToolCall>,
    ) -> Result<Message> {
        ensure!(!calls.is_empty(), "a tool step needs a call");
        let new = NewMessage::assistant(answer.content, answer.reasoning, calls);
        let timestamp = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin tool calls")?;
        let turn = read_running_turn(&mut transaction, turn_id).await?;
        let message = insert_message(&mut transaction, &turn.thread_id, &new, None, timestamp);
        let message = message.await?;
        transaction.commit().await.context("commit tool calls")?;
        Ok(message)
    }

    /// Stores what one call of the newest tool step returned.
    pub async fn record_tool_result(
        &self,
        turn_id: &str,
        call_id: &str,
        content: String,
    ) -> Result<Message> {
        let timestamp = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin tool result")?;
        let turn = read_running_turn(&mut transaction, turn_id).await?;
        let open = open_calls(&mut transaction, &turn.thread_id).await?;
        ensure!(
            open.iter().any(|open| open == call_id),
            "tool call {call_id} is not waiting for a result"
        );
        let new = NewMessage::tool(call_id.to_owned(), content);
        let message = insert_message(&mut transaction, &turn.thread_id, &new, None, timestamp);
        let message = message.await?;
        transaction.commit().await.context("commit tool result")?;
        Ok(message)
    }

    /// Records how a running turn ended, together with its answer message.
    pub async fn finish_turn(&self, turn_id: &str, outcome: TurnOutcome) -> Result<Turn> {
        let (status, answer, error) = match outcome {
            TurnOutcome::Completed(answer) => (TurnStatus::Completed, Some(answer), None),
            TurnOutcome::Interrupted(answer) => (TurnStatus::Interrupted, answer, None),
            TurnOutcome::Failed(error) => (TurnStatus::Failed, None, Some(error)),
        };
        let timestamp = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin turn finish")?;
        let turn = read_running_turn(&mut transaction, turn_id).await?;
        let unfinished = "no result: the turn ended before this call finished";
        close_open_calls(&mut transaction, &turn.thread_id, unfinished, timestamp).await?;
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
        Ok(turn)
    }

    /// A process that stopped mid-turn leaves it running; mark such turns interrupted.
    /// A tool that was running then may or may not have taken effect, and its result says so.
    pub async fn interrupt_running_turns(&self, thread_id: &str) -> Result<()> {
        let timestamp = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin recovery")?;
        let lost = "no result: the process stopped while this call ran, so it may or may not \
                    have taken effect";
        close_open_calls(&mut transaction, thread_id, lost, timestamp).await?;
        sqlx::query(
            "UPDATE turns SET status = 'interrupted', finished_at = MAX(started_at, ?)
             WHERE thread_id = ? AND status = 'running'",
        )
        .bind(timestamp)
        .bind(thread_id)
        .execute(&mut *transaction)
        .await
        .context("interrupt running turns")?;
        transaction.commit().await.context("commit recovery")?;
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

/// Calls of the thread's newest tool step that have no result yet, in call order. Older
/// steps are always complete: the next step only starts once every call has a result.
async fn open_calls(
    transaction: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
) -> Result<Vec<String>> {
    let step = sqlx::query(
        "SELECT sequence, tool_calls_json FROM messages
         WHERE thread_id = ? AND tool_calls_json IS NOT NULL ORDER BY sequence DESC LIMIT 1",
    )
    .bind(thread_id)
    .fetch_optional(&mut **transaction)
    .await
    .context("load the newest tool step")?;
    let Some(step) = step else {
        return Ok(Vec::new());
    };
    let sequence: i64 = step.try_get("sequence")?;
    let calls: String = step.try_get("tool_calls_json")?;
    let calls: Vec<ToolCall> = serde_json::from_str(&calls).context("decode tool calls")?;
    let answered: Vec<String> = sqlx::query_scalar(
        "SELECT tool_call_id FROM messages
         WHERE thread_id = ? AND role = 'tool' AND sequence > ?",
    )
    .bind(thread_id)
    .bind(sequence)
    .fetch_all(&mut **transaction)
    .await
    .context("load tool results")?;
    Ok(calls
        .into_iter()
        .map(|call| call.id)
        .filter(|id| !answered.contains(id))
        .collect())
}

async fn close_open_calls(
    transaction: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    result: &str,
    timestamp: i64,
) -> Result<()> {
    for call_id in open_calls(transaction, thread_id).await? {
        let new = NewMessage::tool(call_id, result.to_owned());
        insert_message(transaction, thread_id, &new, None, timestamp).await?;
    }
    Ok(())
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

    fn call(id: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: "echo".into(),
            arguments: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn calls_are_stored_before_their_results() {
        let state = thread("turn_step").await;
        send(&state, "a").await;
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        let calls = vec![call("call_1"), call("call_2")];
        let answer = answer("", Some("think"));
        state
            .record_tool_calls(&turn.turn_id, answer, calls)
            .await
            .unwrap();
        let id = &turn.turn_id;
        state
            .record_tool_result(id, "call_1", "pong".into())
            .await
            .unwrap();
        let again = state.record_tool_result(id, "call_1", "twice".into()).await;
        assert!(again.is_err(), "a call gets one result");
        let unknown = state.record_tool_result(id, "call_9", "?".into()).await;
        assert!(unknown.is_err());

        let messages = state.list_messages("thread").await.unwrap();
        assert_eq!(messages[1].role, MessageRole::Assistant);
        assert_eq!(messages[1].reasoning.as_deref(), Some("think"));
        assert_eq!(messages[1].tool_calls.len(), 2);
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(messages.len(), 3, "call_2 is still running");
    }

    #[tokio::test]
    async fn a_call_left_running_gets_a_result_on_recovery() {
        let state = thread("turn_lost_call").await;
        send(&state, "a").await;
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        let calls = vec![call("call_1")];
        let answer = answer("", None);
        state
            .record_tool_calls(&turn.turn_id, answer, calls)
            .await
            .unwrap();

        state.interrupt_running_turns("thread").await.unwrap();

        let messages = state.list_messages("thread").await.unwrap();
        let result = messages.last().unwrap();
        assert_eq!(result.tool_call_id.as_deref(), Some("call_1"));
        assert!(
            result.content.contains("may or may not"),
            "{}",
            result.content
        );
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
        state.start_turn("thread").await.unwrap().unwrap();

        state.interrupt_running_turns("thread").await.unwrap();

        // Only one turn per thread may be running, so a new one proves the old one ended.
        send(&state, "b").await;
        assert!(state.start_turn("thread").await.unwrap().is_some());
    }
}
