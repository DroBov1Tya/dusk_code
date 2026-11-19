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
    failure::{TurnFailure, columns},
    messages::{NewMessage, insert_message, read_tokens},
    resume::LOST,
    timestamp_ms,
    waits::decode_wait,
};
use crate::providers::ToolCall;

pub use dusk_protocol::thread::{Turn, TurnSpend, TurnStatus};

/// The columns of a turn: its row and what its model calls spent, read beside it so every
/// turn loaded carries its totals. Steps are the calls that answered; tokens and cost
/// count every call of the turn, a compaction inside it too.
macro_rules! turn_row {
    () => {
        "*, (SELECT COUNT(*) FROM model_usage u WHERE u.thread_id = turns.thread_id
                AND u.purpose = 'answer' AND u.turn_id = turns.turn_id) AS steps,
            (SELECT COALESCE(SUM(u.prompt_tokens + u.completion_tokens), 0) FROM model_usage u
                WHERE u.thread_id = turns.thread_id AND u.turn_id = turns.turn_id) AS tokens,
            (SELECT COALESCE(SUM(u.cost_micro_usd), 0) FROM model_usage u
                WHERE u.thread_id = turns.thread_id AND u.turn_id = turns.turn_id)
                AS cost_micro_usd"
    };
}
pub(super) use turn_row;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TurnOutcome {
    Completed(TurnAnswer),
    Interrupted(Option<TurnAnswer>),
    Failed(TurnFailure),
}

impl TurnOutcome {
    /// A turn that failed on something other than a model call.
    pub fn failed(error: impl Into<String>) -> Self {
        Self::Failed(TurnFailure {
            error: error.into(),
            provider: None,
        })
    }
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
        let row = sqlx::query(concat!(
            "INSERT INTO turns (turn_id, thread_id, last_sequence, status, started_at)
             SELECT ?, ?, MAX(sequence), 'running', ? FROM messages
             WHERE thread_id = ? AND (role = 'user' OR from_job_id IS NOT NULL)
               AND sequence > (SELECT COALESCE(MAX(last_sequence), 0) FROM turns WHERE thread_id = ?)
             HAVING COUNT(*) > 0
             RETURNING ",
            turn_row!()
        )
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

    /// Counts user messages that joined a running turn, such as messages from other
    /// agents taken in between steps, as answered by it, so no second turn takes them.
    pub async fn extend_turn(&self, turn_id: &str, through: u64) -> Result<()> {
        let through = i64::try_from(through).context("sequence exceeds SQLite range")?;
        sqlx::query("UPDATE turns SET last_sequence = MAX(last_sequence, ?) WHERE turn_id = ?")
            .bind(through)
            .bind(turn_id)
            .execute(&self.pool)
            .await
            .context("extend turn")?;
        Ok(())
    }

    /// Stores the agent's message with its tool calls, before any of them runs.
    pub async fn record_tool_calls(
        &self,
        turn_id: &str,
        answer: TurnAnswer,
        calls: Vec<ToolCall>,
    ) -> Result<Message> {
        ensure!(!calls.is_empty(), "a tool step needs a call");
        let new = NewMessage::assistant(answer.content, answer.reasoning, calls.clone());
        let timestamp = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin tool calls")?;
        let turn = read_running_turn(&mut transaction, turn_id).await?;
        let message = insert_message(&mut transaction, &turn.thread_id, &new, None, timestamp);
        let message = message.await?;
        // A paused contract still owns its evidence. Work performed meanwhile
        // must invalidate earlier checks before a later resume can reuse them.
        let contract: Option<(String, String)> = sqlx::query_as(
            "SELECT revision, criteria_json FROM goals
            WHERE thread_id = ? AND approved_at IS NOT NULL",
        )
        .bind(&turn.thread_id)
        .fetch_optional(&mut *transaction)
        .await?;
        let criteria: Vec<crate::state::GoalCriterion> = contract
            .as_ref()
            .map(|(_, json)| serde_json::from_str(json))
            .transpose()?
            .unwrap_or_default();
        for call in calls {
            if contract.is_some() {
                let approved_check = call.name == "run_command"
                    && criteria.iter().any(|c| {
                        matches!(&c.check, crate::state::GoalCheck::Command { command }
                        if call.arguments["command"].as_str() == Some(command))
                    });
                let read_only = matches!(
                    call.name.as_str(),
                    "goal"
                        | "read_file"
                        | "changes"
                        | "search_files"
                        | "read_history"
                        | "recall"
                        | "project_graph"
                        | "load_skill"
                        | "display_image"
                        | "display_audio"
                        | "show"
                        | "tasks"
                        | "remember"
                        | "ask_user"
                        | "suggest_next"
                        | "jobs"
                );
                if !approved_check && !read_only {
                    sqlx::query("UPDATE goals SET work_epoch = work_epoch + 1 WHERE thread_id = ?")
                        .bind(&turn.thread_id)
                        .execute(&mut *transaction)
                        .await?;
                }
            }
            sqlx::query(
                "INSERT INTO tool_executions (execution_id, turn_id, thread_id, call_id,
                call_message_id, tool_name, arguments_json, goal_revision, goal_epoch)
                VALUES (?, ?, ?, ?, ?, ?, ?, (SELECT revision FROM goals WHERE thread_id = ?
                    AND approved_at IS NOT NULL),
                    (SELECT work_epoch FROM goals WHERE thread_id = ? AND approved_at IS NOT NULL))",
            )
            .bind(Uuid::now_v7().to_string())
            .bind(turn_id)
            .bind(&turn.thread_id)
            .bind(call.id)
            .bind(&message.message_id)
            .bind(call.name)
            .bind(serde_json::to_string(&call.arguments)?)
            .bind(&turn.thread_id)
            .bind(&turn.thread_id)
            .execute(&mut *transaction)
            .await
            .context("record call provenance")?;
        }
        transaction.commit().await.context("commit tool calls")?;
        Ok(message)
    }

    /// Stores what one call of the newest tool step returned.
    #[cfg(test)]
    pub async fn record_tool_result(
        &self,
        turn_id: &str,
        call_id: &str,
        content: String,
    ) -> Result<Message> {
        self.record_tool_result_with_artifacts(turn_id, call_id, content, Vec::new())
            .await
    }

    #[cfg(test)]
    pub async fn record_tool_result_with_artifacts(
        &self,
        turn_id: &str,
        call_id: &str,
        content: String,
        artifacts: Vec<crate::state::artifacts::Card>,
    ) -> Result<Message> {
        self.record_tool_result_with_outcome(turn_id, call_id, content, artifacts, "unknown", None)
            .await
    }

    pub async fn record_tool_result_with_outcome(
        &self,
        turn_id: &str,
        call_id: &str,
        content: String,
        artifacts: Vec<crate::state::artifacts::Card>,
        outcome: &str,
        exit_code: Option<i32>,
    ) -> Result<Message> {
        let timestamp = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin tool result")?;
        let turn = read_running_turn(&mut transaction, turn_id).await?;
        let open = open_calls(&mut transaction, &turn.thread_id).await?;
        ensure!(
            open.iter().any(|open| open.id == call_id),
            "tool call {call_id} is not waiting for a result"
        );
        let new = NewMessage::tool(call_id.to_owned(), content).with_artifacts(artifacts);
        let message = insert_message(&mut transaction, &turn.thread_id, &new, None, timestamp);
        let message = message.await?;
        sqlx::query(
            "UPDATE tool_executions SET result_message_id = ?, outcome = ?, exit_code = ?
            WHERE turn_id = ? AND call_id = ? AND outcome = 'pending'",
        )
        .bind(&message.message_id)
        .bind(outcome)
        .bind(exit_code)
        .bind(turn_id)
        .bind(call_id)
        .execute(&mut *transaction)
        .await?;
        if outcome == "succeeded" {
            sqlx::query("UPDATE goals SET awaiting_user = 1 WHERE thread_id = ? AND revision =
                (SELECT goal_revision FROM tool_executions WHERE turn_id = ? AND call_id = ? AND tool_name = 'ask_user')")
                .bind(&turn.thread_id).bind(turn_id).bind(call_id).execute(&mut *transaction).await?;
        }
        transaction.commit().await.context("commit tool result")?;
        Ok(message)
    }

    /// What the turn's model calls spent so far, for its progress after each call.
    pub async fn turn_spend(&self, turn_id: &str) -> Result<TurnSpend> {
        let row = sqlx::query(concat!(
            "SELECT ",
            turn_row!(),
            " FROM turns WHERE turn_id = ?"
        ))
        .bind(turn_id)
        .fetch_one(&self.pool)
        .await
        .context("load the turn's spend")?;
        Ok(TurnSpend::of(&decode_turn(row)?))
    }

    /// Records how a running turn ended, together with its answer message.
    pub async fn finish_turn(&self, turn_id: &str, outcome: TurnOutcome) -> Result<Turn> {
        let (status, answer, failure) = match outcome {
            TurnOutcome::Completed(answer) => (TurnStatus::Completed, Some(answer), None),
            TurnOutcome::Interrupted(answer) => (TurnStatus::Interrupted, answer, None),
            TurnOutcome::Failed(failure) => (TurnStatus::Failed, None, Some(failure)),
        };
        let (kind, attempts) = match &failure {
            Some(failure) => columns(failure)?,
            None => (None, None),
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
        let row = sqlx::query(concat!(
            "UPDATE turns SET status = ?, response_message_id = ?, error_message = ?,
                error_kind = ?, attempts = ?, finished_at = MAX(started_at, ?),
                waiting_for = NULL, waiting_since = NULL, wake_at = NULL
             WHERE turn_id = ? RETURNING ",
            turn_row!()
        ))
        .bind(status.as_str())
        .bind(message.as_ref().map(|message| &message.message_id))
        .bind(failure.map(|failure| failure.error))
        .bind(kind)
        .bind(attempts)
        .bind(timestamp)
        .bind(turn_id)
        .fetch_one(&mut *transaction)
        .await
        .context("finish turn")?;
        let turn = decode_turn(row)?;
        transaction.commit().await.context("commit turn finish")?;
        Ok(turn)
    }

    /// A process that stopped mid-turn leaves it running; mark such a turn interrupted and
    /// return it. A tool that was running then may have taken effect and may still run,
    /// and its result says so.
    pub async fn interrupt_running_turns(&self, thread_id: &str) -> Result<Option<Turn>> {
        let mut transaction = self.pool.begin().await.context("begin recovery")?;
        let turn = interrupt(&mut transaction, thread_id).await?;
        transaction.commit().await.context("commit recovery")?;
        Ok(turn)
    }
}

/// Interrupts the thread's running turn, if any, inside the caller's transaction.
pub(super) async fn interrupt(
    transaction: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
) -> Result<Option<Turn>> {
    let timestamp = timestamp_ms()?;
    close_open_calls(transaction, thread_id, LOST, timestamp).await?;
    let row = sqlx::query(concat!(
        "UPDATE turns SET status = 'interrupted', finished_at = MAX(started_at, ?),
            waiting_for = NULL, waiting_since = NULL, wake_at = NULL
         WHERE thread_id = ? AND status = 'running' RETURNING ",
        turn_row!()
    ))
    .bind(timestamp)
    .bind(thread_id)
    .fetch_optional(&mut **transaction)
    .await
    .context("interrupt running turns")?;
    row.map(decode_turn).transpose()
}

/// Calls of the thread's newest tool step that have no result yet, in call order. Older
/// steps are always complete: the next step only starts once every call has a result.
pub(super) async fn open_calls(
    transaction: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
) -> Result<Vec<ToolCall>> {
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
        .filter(|call| !answered.contains(&call.id))
        .collect())
}

async fn close_open_calls(
    transaction: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    result: &str,
    timestamp: i64,
) -> Result<()> {
    for call in open_calls(transaction, thread_id).await? {
        close_call(
            transaction,
            thread_id,
            &call.id,
            result,
            "unknown",
            timestamp,
        )
        .await?;
    }
    Ok(())
}

/// Gives an open call the result it ends with when it did not run to its end.
pub(super) async fn close_call(
    transaction: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    call_id: &str,
    result: &str,
    outcome: &str,
    timestamp: i64,
) -> Result<()> {
    let new = NewMessage::tool(call_id.to_owned(), result.to_owned());
    let message = insert_message(transaction, thread_id, &new, None, timestamp).await?;
    sqlx::query(
        "UPDATE tool_executions SET outcome = ?, result_message_id = ?
        WHERE thread_id = ? AND call_id = ? AND outcome = 'pending'",
    )
    .bind(outcome)
    .bind(message.message_id)
    .bind(thread_id)
    .bind(call_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

impl State {
    /// The newest turn of a thread, however it stands; `None` before its first.
    pub async fn last_turn(&self, thread_id: &str) -> Result<Option<Turn>> {
        let row = sqlx::query(concat!(
            "SELECT ",
            turn_row!(),
            " FROM turns WHERE thread_id = ? ORDER BY started_at DESC, turn_id DESC LIMIT 1"
        ))
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await
        .context("load the newest turn")?;
        row.map(decode_turn).transpose()
    }
}

pub(super) async fn read_running_turn(
    transaction: &mut Transaction<'_, Sqlite>,
    turn_id: &str,
) -> Result<Turn> {
    let row = sqlx::query(concat!(
        "SELECT ",
        turn_row!(),
        " FROM turns WHERE turn_id = ? AND status = 'running'"
    ))
    .bind(turn_id)
    .fetch_optional(&mut **transaction)
    .await
    .context("load running turn")?
    .context("turn is not running")?;
    decode_turn(row)
}

pub(super) fn decode_turn(row: SqliteRow) -> Result<Turn> {
    let status: String = row.try_get("status")?;
    let status = match status.as_str() {
        "running" => TurnStatus::Running,
        "completed" => TurnStatus::Completed,
        "interrupted" => TurnStatus::Interrupted,
        "failed" => TurnStatus::Failed,
        _ => bail!("invalid stored turn status"),
    };
    let last_sequence: i64 = row.try_get("last_sequence")?;
    let waiting = decode_wait(&row)?;
    Ok(Turn {
        turn_id: row.try_get("turn_id")?,
        thread_id: row.try_get("thread_id")?,
        last_sequence: last_sequence.try_into().context("negative turn sequence")?,
        response_message_id: row.try_get("response_message_id")?,
        status,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        error_message: row.try_get("error_message")?,
        waiting,
        steps: read_tokens(&row, "steps")?
            .try_into()
            .context("too many steps")?,
        tokens: read_tokens(&row, "tokens")?,
        cost_micro_usd: read_tokens(&row, "cost_micro_usd")?,
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
            result.content.contains("may still be running"),
            "{}",
            result.content
        );
    }
}
