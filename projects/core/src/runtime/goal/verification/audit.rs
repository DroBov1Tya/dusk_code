//! The review that decides whether the evidence of a goal's criteria is enough.

use super::evidence::read_evidence;
use crate::state::{GoalCheck, GoalCriterion, GoalEvidence, State};
use anyhow::Result;
use sqlx::{Row, Sqlite, Transaction};
use std::time::Duration;

pub(super) const REVIEW_TIMEOUT: Duration = Duration::from_secs(5);

/// Pending messages, unconsumed inputs, jobs and descendant agents are checked in
/// the same database transaction as completion. The caller also holds the mailbox
/// lock so a live child's report cannot be halfway through delivery.
pub(super) const BUSY: &str = "WITH RECURSIVE agents(id) AS (
    SELECT id FROM threads WHERE parent_thread_id = ?1
    UNION ALL SELECT t.id FROM threads t JOIN agents a ON t.parent_thread_id = a.id
) SELECT EXISTS(SELECT 1 FROM pending_messages WHERE thread_id = ?1 OR thread_id IN agents)
    OR EXISTS(SELECT 1 FROM turns WHERE (thread_id = ?1 OR thread_id IN agents) AND status = 'running')
    OR EXISTS(SELECT 1 FROM background_jobs WHERE (thread_id = ?1 OR thread_id IN agents) AND status = 'running')
    OR EXISTS(SELECT 1 FROM goals WHERE thread_id = ?1 AND awaiting_user = 1)
    OR EXISTS(SELECT 1 FROM messages m WHERE (m.thread_id = ?1 OR m.thread_id IN agents)
        AND (m.role = 'user' OR m.from_job_id IS NOT NULL)
        AND m.sequence > (SELECT COALESCE(MAX(last_sequence), 0) FROM turns WHERE thread_id = m.thread_id))";

pub(super) type Review<'a> = (Transaction<'a, Sqlite>, Vec<String>, bool);

pub(super) async fn review<'a>(
    state: &'a State,
    thread_id: &str,
    revision: &str,
    attempt: &str,
) -> Result<Option<Review<'a>>> {
    let mut tx = state.pool.begin().await?;
    let busy: bool = sqlx::query_scalar(BUSY)
        .bind(thread_id)
        .fetch_one(&mut *tx)
        .await?;
    if busy {
        return Ok(None);
    }
    let row = sqlx::query(
        "SELECT g.*, t.status AS request_status FROM goals g
        LEFT JOIN turns t ON t.turn_id = g.verification_turn_id AND t.thread_id = g.thread_id
        WHERE g.thread_id = ? AND g.revision = ? AND g.verification_id = ?
            AND g.verification = 'checking' AND g.status = 'active' AND g.approved_at IS NOT NULL",
    )
    .bind(thread_id)
    .bind(revision)
    .bind(attempt)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let criteria: Vec<GoalCriterion> = serde_json::from_str(row.try_get("criteria_json")?)?;
    let evidence: Vec<GoalEvidence> =
        serde_json::from_str(row.try_get::<&str, _>("verification_request")?)?;
    let after: i64 = row.try_get("evidence_after_sequence")?;
    let approved_at: i64 = row.try_get("approved_at")?;
    let epoch: i64 = row.try_get("work_epoch")?;
    let mut gaps = Vec::new();
    let mut only_confirmation = true;
    if row.try_get::<Option<&str>, _>("request_status")? != Some("completed") {
        gaps.push(
            "Verification request turn did not complete successfully; request verification again."
                .into(),
        );
        only_confirmation = false;
    }
    if criteria.is_empty() || evidence.len() != criteria.len() {
        gaps.push("The approved contract or evidence set is incomplete.".into());
        only_confirmation = false;
    }
    for criterion in criteria {
        let submitted: Vec<_> = evidence
            .iter()
            .filter(|e| e.criterion_id == criterion.id)
            .collect();
        if submitted.len() != 1 {
            gaps.push(format!(
                "{}: exactly one saved result is required.",
                criterion.id
            ));
            only_confirmation = false;
            continue;
        }
        let id = &submitted[0].result_id;
        let source =
            read_evidence(&mut tx, thread_id, revision, after, approved_at, epoch, id).await?;
        let Some(source) = source else {
            gaps.push(format!(
                "{}: {id} is missing, foreign, stale, incomplete, cancelled, failed or untyped.",
                criterion.id
            ));
            only_confirmation = false;
            continue;
        };
        match criterion.check {
            GoalCheck::Command { command } => {
                if source.command.as_deref() != Some(command.as_str()) || !source.command_passed {
                    gaps.push(format!(
                        "{}: {id} does not prove exit 0 for the exact approved command: {command}",
                        criterion.id
                    ));
                    only_confirmation = false;
                }
            }
            GoalCheck::UserConfirmation => {
                let confirmed: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM goal_confirmations
                    WHERE thread_id = ? AND revision = ? AND criterion_id = ? AND result_id = ?)",
                )
                .bind(thread_id)
                .bind(revision)
                .bind(&criterion.id)
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
                if !confirmed {
                    gaps.push(format!("{}: user confirmation required after /goal result {id}; /goal confirm {revision} {} {id}", criterion.id, criterion.id));
                }
            }
        }
    }
    Ok(Some((tx, gaps, only_confirmation)))
}
