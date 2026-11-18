//! A running turn that waits: for the user's decision, when a restart cut off a call that
//! may have taken effect, or for a provider that cannot serve it now. The wait is kept
//! with the turn so a restart finds it. Only a running turn waits; whatever ends the turn
//! clears the wait in the same statement.

use std::sync::Once;

use anyhow::{Context, Result, anyhow};
use sqlx::{Row, sqlite::SqliteRow};

use super::{
    State, timestamp_ms,
    turns::{Turn, decode_turn, read_running_turn, turn_row},
};
use crate::providers::ProviderErrorKind;

use dusk_protocol::{model::ProviderWaitKind, thread::TurnWait};

/// `waiting_for` of a turn the user is asked about.
const DECISION: &str = "decision";

/// `waiting_for` of each provider wait, both ways; the column's CHECK names the same.
const PROVIDER_WAITS: [(ProviderWaitKind, &str); 5] = [
    (ProviderWaitKind::RateLimited, "rate_limited"),
    (ProviderWaitKind::Quota, "quota"),
    (ProviderWaitKind::Unreachable, "unreachable"),
    (ProviderWaitKind::TimedOut, "timed_out"),
    (ProviderWaitKind::Overloaded, "overloaded"),
];

impl State {
    /// Lets the running turn sleep until `wake_at` (Unix ms) because the provider cannot
    /// serve it now. A wait right after another keeps when the first began, so the
    /// waits in a row can be bounded together.
    pub async fn wait_for_provider(
        &self,
        turn_id: &str,
        kind: ProviderErrorKind,
        wake_at: i64,
    ) -> Result<Turn> {
        let kind = ProviderWaitKind::try_from(kind)
            .map_err(|kind| anyhow!("a turn does not wait out {kind:?}"))?;
        let row = sqlx::query(concat!(
            "UPDATE turns SET waiting_for = ?, waiting_since = COALESCE(waiting_since, ?),
                wake_at = ?
             WHERE turn_id = ? AND status = 'running' RETURNING ",
            turn_row!()
        ))
        .bind(provider_column(kind)?)
        .bind(timestamp_ms()?)
        .bind(wake_at)
        .bind(turn_id)
        .fetch_optional(&self.pool)
        .await
        .context("let the turn wait for the provider")?
        .context("turn is not running")?;
        decode_turn(row)
    }

    /// Lets the thread's running turn wait for the user to decide how it goes on. A turn
    /// that waits already, for the user or the provider, is returned as it is.
    pub async fn wait_for_decision(&self, thread_id: &str) -> Result<Turn> {
        let since = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin decision wait")?;
        let updated = sqlx::query(concat!(
            "UPDATE turns SET waiting_for = ?, waiting_since = ?
             WHERE thread_id = ? AND status = 'running' AND waiting_for IS NULL RETURNING ",
            turn_row!()
        ))
        .bind(DECISION)
        .bind(since)
        .bind(thread_id)
        .fetch_optional(&mut *transaction)
        .await
        .context("let the turn wait for a decision")?;
        let row = match updated {
            Some(row) => row,
            None => sqlx::query(concat!(
                "SELECT ",
                turn_row!(),
                " FROM turns WHERE thread_id = ? AND status = 'running'"
            ))
            .bind(thread_id)
            .fetch_optional(&mut *transaction)
            .await
            .context("load running turn")?
            .context("thread has no running turn")?,
        };
        let turn = decode_turn(row)?;
        transaction.commit().await.context("commit decision wait")?;
        Ok(turn)
    }

    /// Ends the running turn's wait; a turn that does not wait is returned as it is and
    /// nothing is written.
    pub async fn end_wait(&self, turn_id: &str) -> Result<Turn> {
        let mut transaction = self.pool.begin().await.context("begin wait end")?;
        let updated = sqlx::query(concat!(
            "UPDATE turns SET waiting_for = NULL, waiting_since = NULL, wake_at = NULL
             WHERE turn_id = ? AND status = 'running' AND waiting_for IS NOT NULL
             RETURNING ",
            turn_row!()
        ))
        .bind(turn_id)
        .fetch_optional(&mut *transaction)
        .await
        .context("end the turn's wait")?;
        let turn = match updated {
            Some(row) => decode_turn(row)?,
            None => read_running_turn(&mut transaction, turn_id).await?,
        };
        transaction.commit().await.context("commit wait end")?;
        Ok(turn)
    }
}

fn provider_column(kind: ProviderWaitKind) -> Result<&'static str> {
    PROVIDER_WAITS
        .iter()
        .find_map(|&(known, text)| (known == kind).then_some(text))
        .with_context(|| format!("no column text for a {kind:?} wait"))
}

/// The wait stored in a turn row. One this build cannot read, written by a newer build or
/// left incomplete, is read as no wait, so the turn still loads; it is logged once.
pub(super) fn decode_wait(row: &SqliteRow) -> Result<Option<TurnWait>> {
    let Some(text) = row.try_get::<Option<String>, _>("waiting_for")? else {
        return Ok(None);
    };
    let since: Option<i64> = row.try_get("waiting_since")?;
    let wake_at: Option<i64> = row.try_get("wake_at")?;
    let kind = PROVIDER_WAITS
        .iter()
        .find_map(|&(kind, known)| (known == text).then_some(kind));
    let wait = match (text.as_str(), kind, since, wake_at) {
        (DECISION, _, Some(since), _) => Some(TurnWait::Decision { since }),
        (_, Some(kind), Some(since), Some(wake_at)) => Some(TurnWait::Provider {
            kind,
            since,
            wake_at,
        }),
        _ => None,
    };
    if wait.is_none() {
        static UNREADABLE: Once = Once::new();
        UNREADABLE.call_once(|| {
            tracing::warn!("a turn waits in a way this build cannot read; it counts as not waiting")
        });
    }
    Ok(wait)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        state::TurnOutcome,
        testing::{temp_dir, temp_state},
    };

    async fn running(state: &State) -> Turn {
        state.create_thread("thread", "test").await.unwrap();
        state.add_user_message("thread", "a", "hi").await.unwrap();
        state.start_turn("thread").await.unwrap().unwrap()
    }

    async fn load(state: &State, turn_id: &str) -> Turn {
        let row = sqlx::query(concat!(
            "SELECT ",
            turn_row!(),
            " FROM turns WHERE turn_id = ?"
        ))
        .bind(turn_id)
        .fetch_one(&state.pool)
        .await
        .unwrap();
        decode_turn(row).unwrap()
    }

    /// The three wait columns as stored, whatever the decoder makes of them.
    async fn columns(state: &State, turn_id: &str) -> (Option<String>, Option<i64>, Option<i64>) {
        sqlx::query_as("SELECT waiting_for, waiting_since, wake_at FROM turns WHERE turn_id = ?")
            .bind(turn_id)
            .fetch_one(&state.pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn waits_in_a_row_keep_when_the_first_began() {
        let state = temp_state("wait_provider").await;
        let turn = running(&state).await;
        let id = &turn.turn_id;
        let first = state
            .wait_for_provider(id, ProviderErrorKind::RateLimited, 10)
            .await
            .unwrap();
        let Some(TurnWait::Provider { since, .. }) = first.waiting else {
            panic!("no provider wait: {first:?}");
        };
        // Every kind a wait passes is one the column takes, and reads back as itself.
        for (kind, text) in PROVIDER_WAITS {
            let wake_at = since + 60_000;
            let again = state
                .wait_for_provider(id, kind.into(), wake_at)
                .await
                .unwrap();
            let expected = TurnWait::Provider {
                kind,
                since,
                wake_at,
            };
            assert_eq!(again.waiting, Some(expected));
            assert_eq!(load(&state, id).await, again);
            assert_eq!(columns(&state, id).await.0.as_deref(), Some(text));
        }
        let refused = state
            .wait_for_provider(id, ProviderErrorKind::Auth, 10)
            .await;
        assert!(refused.is_err(), "a refused key is not waited out");
    }

    #[tokio::test]
    async fn a_decision_wait_leaves_a_provider_wait_alone() {
        let state = temp_state("wait_decision").await;
        assert!(state.wait_for_decision("none").await.is_err());
        let turn = running(&state).await;
        let id = &turn.turn_id;
        let asked = state.wait_for_decision("thread").await.unwrap();
        let Some(TurnWait::Decision { since }) = asked.waiting else {
            panic!("no decision wait: {asked:?}");
        };
        // Asked again, as after a second restart: the first wait stands.
        let again = state.wait_for_decision("thread").await.unwrap();
        assert_eq!(again.waiting, Some(TurnWait::Decision { since }));
        let stored = columns(&state, id).await;
        assert_eq!(stored, (Some(DECISION.into()), Some(since), None));

        state.end_wait(id).await.unwrap();
        let sleeping = state
            .wait_for_provider(id, ProviderErrorKind::Quota, 99)
            .await
            .unwrap();
        let unchanged = state.wait_for_decision("thread").await.unwrap();
        assert_eq!(unchanged, sleeping);
        assert_eq!(load(&state, id).await, sleeping);
    }

    #[tokio::test]
    async fn ending_a_wait_clears_it_and_ending_none_changes_nothing() {
        let state = temp_state("wait_end").await;
        let turn = running(&state).await;
        let id = &turn.turn_id;
        let idle = state.end_wait(id).await.unwrap();
        assert_eq!(idle, turn);
        assert_eq!(columns(&state, id).await, (None, None, None));

        state
            .wait_for_provider(id, ProviderErrorKind::Overloaded, 5)
            .await
            .unwrap();
        let woken = state.end_wait(id).await.unwrap();
        assert_eq!(woken, turn);
        assert_eq!(columns(&state, id).await, (None, None, None));
    }

    #[tokio::test]
    async fn a_turn_that_ends_while_waiting_keeps_no_wait() {
        let state = temp_state("wait_then_end").await;
        let turn = running(&state).await;
        state.wait_for_decision("thread").await.unwrap();
        let ended = state
            .finish_turn(&turn.turn_id, TurnOutcome::Interrupted(None))
            .await
            .unwrap();
        assert_eq!(ended.waiting, None);
        assert_eq!(columns(&state, &turn.turn_id).await, (None, None, None));
        // A wait asked of a turn that ended is refused, and so is ending one.
        let late = state
            .wait_for_provider(&turn.turn_id, ProviderErrorKind::Quota, 5)
            .await;
        assert!(late.is_err());
        assert!(state.end_wait(&turn.turn_id).await.is_err());

        state
            .add_user_message("thread", "b", "again")
            .await
            .unwrap();
        let next = state.start_turn("thread").await.unwrap().unwrap();
        state
            .wait_for_provider(&next.turn_id, ProviderErrorKind::Unreachable, 5)
            .await
            .unwrap();
        state.interrupt_running_turns("thread").await.unwrap();
        assert_eq!(columns(&state, &next.turn_id).await, (None, None, None));
        assert_eq!(load(&state, &next.turn_id).await.waiting, None);
    }

    #[tokio::test]
    async fn the_database_refuses_a_wait_off_a_running_turn() {
        let state = temp_state("wait_trigger").await;
        let turn = running(&state).await;
        let id = &turn.turn_id;
        // The column takes only the waits it names.
        let unknown = sqlx::query("UPDATE turns SET waiting_for = 'later' WHERE turn_id = ?")
            .bind(id)
            .execute(&state.pool)
            .await;
        assert!(unknown.is_err(), "the column CHECK holds");
        state.wait_for_decision("thread").await.unwrap();
        let interrupt = "UPDATE turns SET status = 'interrupted', finished_at = started_at
                         WHERE turn_id = ?";
        let ended = sqlx::query(interrupt).bind(id).execute(&state.pool).await;
        let error = ended.unwrap_err().to_string();
        assert!(error.contains("only a running turn waits"), "{error}");
        state.end_wait(id).await.unwrap();
        sqlx::query(interrupt)
            .bind(id)
            .execute(&state.pool)
            .await
            .unwrap();
        let late = sqlx::query("UPDATE turns SET waiting_for = 'quota' WHERE turn_id = ?")
            .bind(id)
            .execute(&state.pool)
            .await;
        assert!(late.is_err(), "a turn that ended does not start to wait");
        assert_eq!(columns(&state, id).await, (None, None, None));
    }

    #[tokio::test]
    async fn a_wait_survives_a_restart() {
        let path = temp_dir("wait_restart").join("state.db");
        let state = State::open(&path).await.unwrap();
        let turn = running(&state).await;
        let waiting = state
            .wait_for_provider(&turn.turn_id, ProviderErrorKind::TimedOut, 1_234)
            .await
            .unwrap();
        state.close().await;
        let reopened = State::open(&path).await.unwrap();
        assert_eq!(load(&reopened, &turn.turn_id).await, waiting);
        reopened.close().await;
    }

    #[tokio::test]
    async fn a_wait_this_build_cannot_read_is_no_wait() {
        let state = temp_state("wait_unknown").await;
        let turn = running(&state).await;
        // As a newer build with more waits, or one that left a wait half written.
        for (waiting_for, wake_at) in [("later", Some(5)), ("quota", None)] {
            let mut connection = state.pool.acquire().await.unwrap();
            sqlx::raw_sql("PRAGMA ignore_check_constraints = ON")
                .execute(&mut *connection)
                .await
                .unwrap();
            sqlx::query(
                "UPDATE turns SET waiting_for = ?, waiting_since = 1, wake_at = ?
                 WHERE turn_id = ?",
            )
            .bind(waiting_for)
            .bind(wake_at)
            .bind(&turn.turn_id)
            .execute(&mut *connection)
            .await
            .unwrap();
            sqlx::raw_sql("PRAGMA ignore_check_constraints = OFF")
                .execute(&mut *connection)
                .await
                .unwrap();
            drop(connection);
            assert_eq!(load(&state, &turn.turn_id).await.waiting, None);
        }
    }
}
