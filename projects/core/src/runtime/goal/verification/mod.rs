use crate::state::{Goal, State};
use anyhow::Result;
use std::time::Duration;

mod audit;
mod evidence;
#[cfg(test)]
mod tests;

use audit::{BUSY, REVIEW_TIMEOUT, review};

pub async fn verify(state: &State, thread_id: &str) -> Result<Option<Goal>> {
    let Some((revision, attempt)) = begin(state, thread_id).await? else {
        return Ok(None);
    };
    finish_review(state, thread_id, revision, attempt, REVIEW_TIMEOUT).await
}

async fn begin(state: &State, thread_id: &str) -> Result<Option<(String, String)>> {
    let mut tx = state.pool.begin().await?;
    let busy: bool = sqlx::query_scalar(BUSY)
        .bind(thread_id)
        .fetch_one(&mut *tx)
        .await?;
    if busy {
        return Ok(None);
    }
    let started: Option<(String, String)> = sqlx::query_as(
        "UPDATE goals SET verification = 'checking', verification_attempts = verification_attempts + 1
         WHERE thread_id = ? AND status = 'active' AND approved_at IS NOT NULL
            AND verification = 'pending' AND verification_attempts < 3
         RETURNING revision, verification_id")
        .bind(thread_id).fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    Ok(started)
}

async fn finish_review(
    state: &State,
    thread_id: &str,
    revision: String,
    attempt: String,
    timeout: Duration,
) -> Result<Option<Goal>> {
    let reviewed =
        tokio::time::timeout(timeout, review(state, thread_id, &revision, &attempt)).await;
    match reviewed {
        Ok(Ok(Some((mut tx, gaps, needs_user)))) => {
            let verification = if gaps.is_empty() {
                "passed"
            } else if needs_user {
                "needs_user"
            } else {
                "gaps"
            };
            let note = if gaps.is_empty() {
                "All mandatory criteria passed the approved evidence checks. Command success proves only the approved exit check; semantic criteria were confirmed by the user.".to_owned()
            } else {
                gaps.join("\n")
            };
            // Review and this update share one transaction. No timeout or inbox race
            // cancels a commit after a successful review; writes remain atomic.
            sqlx::query("UPDATE goals SET verification = ?, verification_note = ?,
                status = CASE WHEN ? = 'passed' THEN 'done' ELSE status END, updated_at = ?
                WHERE thread_id = ? AND revision = ? AND verification_id = ?
                    AND verification = 'checking' AND status = 'active' AND approved_at IS NOT NULL")
                .bind(verification).bind(note).bind(verification).bind(crate::state::timestamp_ms()?)
                .bind(thread_id).bind(&revision).bind(&attempt).execute(&mut *tx).await?;
            tx.commit().await?;
        }
        Ok(Ok(None)) => {
            // A durable dependency appeared between attempts; retry after delivery.
            sqlx::query("UPDATE goals SET verification = 'pending', verification_attempts = MAX(0, verification_attempts - 1)
                WHERE thread_id = ? AND revision = ? AND verification_id = ? AND verification = 'checking'")
                .bind(thread_id).bind(&revision).bind(&attempt).execute(&state.pool).await?;
            return Ok(None);
        }
        error => {
            let note = match error {
                Err(_) => "Verification timed out; no pass was recorded.".to_owned(),
                Ok(Err(error)) => format!("Verification failed; no pass was recorded: {error:#}"),
                _ => unreachable!(),
            };
            sqlx::query("UPDATE goals SET verification = 'gaps', verification_note = ?
                WHERE thread_id = ? AND revision = ? AND verification_id = ? AND verification = 'checking'")
                .bind(note).bind(thread_id).bind(&revision).bind(&attempt).execute(&state.pool).await?;
        }
    }
    state.goal(thread_id).await
}
