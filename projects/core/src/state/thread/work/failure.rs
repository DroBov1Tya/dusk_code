//! Why a turn failed, kept with the turn so a chat opened again shows the row it showed
//! when the turn ended.

use anyhow::{Context, Result};
use sqlx::Row;

use super::State;
use crate::providers::ProviderErrorKind;

pub use dusk_protocol::{model::ProviderFailure, thread::TurnFailure};

/// The columns `failure` is stored in: the kind by its name, and the attempts.
pub(super) fn columns(failure: &TurnFailure) -> Result<(Option<String>, Option<i64>)> {
    let Some(ProviderFailure { kind, attempts }) = failure.provider else {
        return Ok((None, None));
    };
    let name = match serde_json::to_value(kind)? {
        serde_json::Value::String(name) => name,
        _ => anyhow::bail!("a failure kind is not named by a string"),
    };
    Ok((Some(name), Some(attempts.into())))
}

impl State {
    /// Why the thread's newest turn failed; none when it did not, or there is none.
    pub async fn last_failure(&self, thread_id: &str) -> Result<Option<TurnFailure>> {
        let row = sqlx::query(
            "SELECT status, error_message, error_kind, attempts FROM turns WHERE thread_id = ?
             ORDER BY started_at DESC, turn_id DESC LIMIT 1",
        )
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await
        .context("load the newest turn")?;
        let Some(row) = row else {
            return Ok(None);
        };
        let status: String = row.try_get("status")?;
        let error: Option<String> = row.try_get("error_message")?;
        let (Some(error), "failed") = (error, status.as_str()) else {
            return Ok(None);
        };
        let kind: Option<String> = row.try_get("error_kind")?;
        let attempts: Option<i64> = row.try_get("attempts")?;
        // A kind a newer build stored and this one does not know leaves the plain error.
        let kind = kind.and_then(|name| {
            serde_json::from_value::<ProviderErrorKind>(serde_json::Value::String(name)).ok()
        });
        let attempts = attempts.and_then(|attempts| u32::try_from(attempts).ok());
        let provider = kind
            .zip(attempts)
            .map(|(kind, attempts)| ProviderFailure { kind, attempts });
        Ok(Some(TurnFailure { error, provider }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{state::TurnOutcome, testing::temp_state};

    #[tokio::test]
    async fn a_failed_turn_is_read_back_as_it_ended_until_a_later_one_starts() {
        let state = temp_state("turn_failure").await;
        state.create_thread("thread", "test").await.unwrap();
        assert_eq!(state.last_failure("thread").await.unwrap(), None);
        let failure = TurnFailure {
            error: "could not reach api.example.com".into(),
            provider: Some(ProviderFailure {
                kind: ProviderErrorKind::Unreachable,
                attempts: 9,
            }),
        };
        let plain = TurnFailure {
            error: "cannot store the tool calls".into(),
            provider: None,
        };
        for (uid, ended) in [("a", &failure), ("b", &plain)] {
            state.add_user_message("thread", uid, uid).await.unwrap();
            let turn = state.start_turn("thread").await.unwrap().unwrap();
            let outcome = TurnOutcome::Failed(ended.clone());
            state.finish_turn(&turn.turn_id, outcome).await.unwrap();
            assert_eq!(
                state.last_failure("thread").await.unwrap().as_ref(),
                Some(ended)
            );
        }
        state.add_user_message("thread", "c", "c").await.unwrap();
        state.start_turn("thread").await.unwrap().unwrap();
        assert_eq!(state.last_failure("thread").await.unwrap(), None);
    }
}
