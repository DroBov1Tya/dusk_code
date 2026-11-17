//! What a restart finds of a turn that was running, and the writes that settle it. Each
//! write is one transaction: a crash leaves the turn as it was before it or after it.

use anyhow::{Context, Result, ensure};
use dusk_protocol::thread::TurnWait;

use super::{
    State, messages, timestamp_ms,
    turns::{LOST, Turn, close_call, decode_turn, interrupt, open_calls, read_running_turn},
};
use crate::providers::ToolCall;

/// What the calls after a cut-off one get: they never started.
const NOT_RUN: &str = "not run: an earlier call of this step was cut off by a restart";

/// A turn a stopped process left running.
#[derive(Debug, Clone, PartialEq)]
pub struct Unfinished {
    pub turn: Turn,
    /// Calls of the newest step without a result, in call order. Calls run one by one,
    /// so only the first can have started.
    pub open: Vec<ToolCall>,
    /// The newest time anything of the turn was written, in Unix ms.
    pub active_at: i64,
}

/// How a decided turn goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settle<'a> {
    /// Nothing was cut off: the turn goes on as it stood.
    AsItStood,
    /// The first open call was cut off: it gets the lost result and the calls after it
    /// are not run; `note` then joins the thread for the model.
    CutOff { note: Option<&'a str> },
}

impl State {
    /// The thread's running turn with its open calls; `None` when no turn runs.
    pub async fn unfinished_turn(&self, thread_id: &str) -> Result<Option<Unfinished>> {
        let mut transaction = self.pool.begin().await.context("begin unfinished turn")?;
        let row = sqlx::query("SELECT * FROM turns WHERE thread_id = ? AND status = 'running'")
            .bind(thread_id)
            .fetch_optional(&mut *transaction)
            .await
            .context("load running turn")?;
        let Some(row) = row else {
            return Ok(None);
        };
        let turn = decode_turn(row)?;
        let open = open_calls(&mut transaction, thread_id).await?;
        let written: Option<i64> =
            sqlx::query_scalar("SELECT MAX(created_at) FROM messages WHERE thread_id = ?")
                .bind(thread_id)
                .fetch_one(&mut *transaction)
                .await
                .context("load the newest message time")?;
        transaction
            .commit()
            .await
            .context("commit unfinished turn")?;
        let active_at = written.map_or(turn.started_at, |at| at.max(turn.started_at));
        Ok(Some(Unfinished {
            turn,
            open,
            active_at,
        }))
    }

    /// Applies the user's decision to go on with a turn that waits for it: settles the
    /// cut-off call, if any, ends the wait and drops the answer, so it never joins the
    /// history. A decision applied already, or a turn that does not wait, is refused.
    pub async fn settle_decision(
        &self,
        turn_id: &str,
        answer: i64,
        settle: Settle<'_>,
    ) -> Result<Turn> {
        let timestamp = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin decision")?;
        let turn = read_running_turn(&mut transaction, turn_id).await?;
        ensure!(
            matches!(turn.waiting, Some(TurnWait::Decision { .. })),
            "turn {turn_id} does not wait for a decision"
        );
        take_answer(&mut transaction, answer).await?;
        if let Settle::CutOff { note } = settle {
            let open = open_calls(&mut transaction, &turn.thread_id).await?;
            for (index, call) in open.iter().enumerate() {
                let (result, outcome) = match index {
                    0 => (LOST, "unknown"),
                    _ => (NOT_RUN, "cancelled"),
                };
                let thread = &turn.thread_id;
                close_call(
                    &mut transaction,
                    thread,
                    &call.id,
                    result,
                    outcome,
                    timestamp,
                )
                .await?;
            }
            if let Some(note) = note {
                let new = messages::NewMessage::system(note);
                let insert = messages::insert_message(
                    &mut transaction,
                    &turn.thread_id,
                    &new,
                    None,
                    timestamp,
                );
                insert.await?;
            }
        }
        let row = sqlx::query(
            "UPDATE turns SET waiting_for = NULL, waiting_since = NULL, wake_at = NULL
             WHERE turn_id = ? RETURNING *",
        )
        .bind(turn_id)
        .fetch_one(&mut *transaction)
        .await
        .context("end the decision wait")?;
        let turn = decode_turn(row)?;
        transaction.commit().await.context("commit decision")?;
        Ok(turn)
    }

    /// The user chose to stop the turn a restart cut off: it is interrupted as any turn a
    /// restart finds, and the answer is dropped with it.
    pub async fn stop_decided(&self, thread_id: &str, answer: i64) -> Result<Option<Turn>> {
        let mut transaction = self.pool.begin().await.context("begin decided stop")?;
        take_answer(&mut transaction, answer).await?;
        let turn = interrupt(&mut transaction, thread_id).await?;
        transaction.commit().await.context("commit decided stop")?;
        Ok(turn)
    }

    /// Agent threads with a running turn: a stopped process left them at work.
    pub async fn unfinished_agents(&self) -> Result<Vec<String>> {
        sqlx::query_scalar(
            "SELECT turns.thread_id FROM turns JOIN threads ON threads.id = turns.thread_id
             WHERE turns.status = 'running' AND threads.parent_thread_id IS NOT NULL",
        )
        .fetch_all(&self.pool)
        .await
        .context("list agents left at work")
    }
}

/// Removes the answer from the waiting messages; one already taken is refused, so a
/// decision is applied once.
async fn take_answer(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    answer: i64,
) -> Result<()> {
    let removed = sqlx::query("DELETE FROM pending_messages WHERE pending_id = ?")
        .bind(answer)
        .execute(&mut **transaction)
        .await
        .context("take the answer")?;
    ensure!(removed.rows_affected() == 1, "the answer was taken already");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Message, MessageRole, TurnAnswer};
    use crate::testing::temp_state;

    /// A turn whose step called `a` and `b`, neither with a result yet.
    async fn cut_off(name: &str) -> (State, Turn) {
        let state = temp_state(name).await;
        state.create_thread("thread", "test").await.unwrap();
        state.add_user_message("thread", "u", "go").await.unwrap();
        let turn = state.start_turn("thread").await.unwrap().unwrap();
        let call = |id: &str| ToolCall {
            id: id.into(),
            name: "echo".into(),
            arguments: serde_json::json!({}),
        };
        let answer = TurnAnswer {
            content: String::new(),
            reasoning: None,
        };
        let calls = vec![call("a"), call("b")];
        let id = &turn.turn_id;
        state.record_tool_calls(id, answer, calls).await.unwrap();
        (state, turn)
    }

    fn results(messages: &[Message]) -> Vec<(&str, &str)> {
        messages
            .iter()
            .filter_map(|m| Some((m.tool_call_id.as_deref()?, m.content.as_str())))
            .collect()
    }

    #[tokio::test]
    async fn a_decision_is_applied_once_together_with_its_results() {
        let (state, turn) = cut_off("settle_once").await;
        let id = &turn.turn_id;
        let unfinished = state.unfinished_turn("thread").await.unwrap().unwrap();
        let open: Vec<&str> = unfinished.open.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(open, ["a", "b"]);
        assert!(unfinished.active_at >= turn.started_at);
        let answer = state.add_pending("thread", "Continue").await.unwrap();
        let settle = Settle::CutOff {
            note: Some("check"),
        };
        let before = state.list_messages("thread").await.unwrap();

        // A turn that does not wait takes no decision, and nothing is written.
        assert!(
            state
                .settle_decision(id, answer.pending_id, settle)
                .await
                .is_err()
        );
        assert_eq!(state.list_messages("thread").await.unwrap(), before);
        assert_eq!(state.pending_messages("thread").await.unwrap().len(), 1);

        state.wait_for_decision("thread").await.unwrap();
        let settled = state.settle_decision(id, answer.pending_id, settle);
        assert_eq!(settled.await.unwrap().waiting, None);
        let after = state.list_messages("thread").await.unwrap();
        assert_eq!(results(&after), [("a", LOST), ("b", NOT_RUN)]);
        let last = after.last().unwrap();
        assert_eq!(
            (last.role, last.content.as_str()),
            (MessageRole::System, "check")
        );
        assert!(state.pending_messages("thread").await.unwrap().is_empty());
        let open = state.unfinished_turn("thread").await.unwrap().unwrap().open;
        assert!(open.is_empty());

        // Applied again, as after a crash right after the first: refused, nothing written.
        state.wait_for_decision("thread").await.unwrap();
        assert!(
            state
                .settle_decision(id, answer.pending_id, settle)
                .await
                .is_err()
        );
        assert_eq!(state.list_messages("thread").await.unwrap(), after);
    }

    #[tokio::test]
    async fn a_decided_stop_drops_the_answer_with_the_turn() {
        let (state, turn) = cut_off("settle_stop").await;
        let answer = state.add_pending("thread", "Stop").await.unwrap();
        state.wait_for_decision("thread").await.unwrap();

        let stopped = state.stop_decided("thread", answer.pending_id).await;
        let stopped = stopped.unwrap().unwrap();
        assert_eq!(stopped.turn_id, turn.turn_id);
        assert_eq!(stopped.status, crate::state::TurnStatus::Interrupted);
        assert_eq!(stopped.waiting, None);
        let messages = state.list_messages("thread").await.unwrap();
        assert_eq!(results(&messages), [("a", LOST), ("b", LOST)]);
        assert!(state.pending_messages("thread").await.unwrap().is_empty());
        assert!(
            state
                .stop_decided("thread", answer.pending_id)
                .await
                .is_err()
        );
        assert_eq!(state.list_messages("thread").await.unwrap(), messages);
    }
}
