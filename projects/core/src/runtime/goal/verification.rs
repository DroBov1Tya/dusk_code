//! Deterministic evidence review. Only this runtime service writes `done`.
//! It reads saved outcomes, never executes commands or asks an executor for a verdict.

use anyhow::{Context, Result};
use sqlx::{Row, Sqlite, Transaction};
use std::time::Duration;

use crate::state::{Goal, GoalCheck, GoalCriterion, GoalEvidence, State};

const REVIEW_TIMEOUT: Duration = Duration::from_secs(5);

/// Pending messages, unconsumed inputs, jobs and descendant agents are checked in
/// the same database transaction as completion. The caller also holds the mailbox
/// lock so a live child's report cannot be halfway through delivery.
const BUSY: &str = "WITH RECURSIVE agents(id) AS (
    SELECT id FROM threads WHERE parent_thread_id = ?1
    UNION ALL SELECT t.id FROM threads t JOIN agents a ON t.parent_thread_id = a.id
) SELECT EXISTS(SELECT 1 FROM pending_messages WHERE thread_id = ?1 OR thread_id IN agents)
    OR EXISTS(SELECT 1 FROM turns WHERE (thread_id = ?1 OR thread_id IN agents) AND status = 'running')
    OR EXISTS(SELECT 1 FROM background_jobs WHERE (thread_id = ?1 OR thread_id IN agents) AND status = 'running')
    OR EXISTS(SELECT 1 FROM goals WHERE thread_id = ?1 AND awaiting_user = 1)
    OR EXISTS(SELECT 1 FROM messages m WHERE (m.thread_id = ?1 OR m.thread_id IN agents)
        AND (m.role = 'user' OR m.from_job_id IS NOT NULL)
        AND m.sequence > (SELECT COALESCE(MAX(last_sequence), 0) FROM turns WHERE thread_id = m.thread_id))";

pub(super) async fn verify(state: &State, thread_id: &str) -> Result<Option<Goal>> {
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

type Review<'a> = (Transaction<'a, Sqlite>, Vec<String>, bool);

async fn review<'a>(
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

struct SavedEvidence {
    command: Option<String>,
    command_passed: bool,
}

async fn read_evidence(
    tx: &mut Transaction<'_, Sqlite>,
    thread_id: &str,
    revision: &str,
    after: i64,
    approved_at: i64,
    epoch: i64,
    id: &str,
) -> Result<Option<SavedEvidence>> {
    if let Some(id) = id.strip_prefix("tool:") {
        let row = sqlx::query(
            "SELECT e.tool_name, e.arguments_json, e.exit_code FROM tool_executions e
            JOIN messages m ON m.message_id = e.result_message_id AND m.thread_id = e.thread_id
            JOIN turns t ON t.turn_id = e.turn_id AND t.thread_id = e.thread_id
            WHERE e.thread_id = ? AND e.goal_revision = ? AND e.result_message_id = ?
                AND e.outcome = 'succeeded' AND t.status = 'completed' AND m.sequence > ? AND e.goal_epoch = ?",
        )
        .bind(thread_id)
        .bind(revision)
        .bind(id)
        .bind(after)
        .bind(epoch)
        .fetch_optional(&mut **tx)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let arguments: serde_json::Value =
            serde_json::from_str(row.try_get("arguments_json")?).context("saved tool arguments")?;
        let shell = row.try_get::<&str, _>("tool_name")? == "run_command"
            && !arguments
                .get("background")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        Ok(Some(SavedEvidence {
            command: shell
                .then(|| arguments["command"].as_str().map(str::to_owned))
                .flatten(),
            command_passed: shell && row.try_get::<Option<i64>, _>("exit_code")? == Some(0),
        }))
    } else if let Some(id) = id.strip_prefix("job:") {
        let command: Option<String> = sqlx::query_scalar("SELECT command FROM background_jobs
            WHERE thread_id = ? AND goal_revision = ? AND job_id = ? AND status = 'completed' AND created_at >= ? AND goal_epoch = ?")
            .bind(thread_id).bind(revision).bind(id).bind(approved_at).bind(epoch).fetch_optional(&mut **tx).await?;
        Ok(command.map(|command| SavedEvidence {
            command: Some(command),
            command_passed: true,
        }))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        providers::ToolCall,
        state::{GoalStatus, Turn, TurnAnswer, TurnOutcome},
        testing::temp_state,
    };
    use serde_json::json;
    use uuid::Uuid;

    fn criterion(check: GoalCheck) -> GoalCriterion {
        GoalCriterion {
            id: "check".into(),
            description: "accepted behavior".into(),
            check,
        }
    }
    fn answer() -> TurnAnswer {
        TurnAnswer {
            content: "reported complete".into(),
            reasoning: None,
        }
    }
    async fn approved(name: &str, check: GoalCheck) -> (State, Goal) {
        let state = temp_state(name).await;
        state.create_thread("thread", "test").await.unwrap();
        let goal = state
            .set_goal("thread", "implement behavior")
            .await
            .unwrap();
        let goal = state
            .propose_goal("thread", &goal.revision, vec![criterion(check)])
            .await
            .unwrap();
        let goal = state.approve_goal("thread", &goal.revision).await.unwrap();
        (state, goal)
    }
    async fn turn(state: &State) -> Turn {
        state
            .add_user_message("thread", &Uuid::now_v7().to_string(), "work")
            .await
            .unwrap();
        state.start_turn("thread").await.unwrap().unwrap()
    }
    async fn result(
        state: &State,
        name: &str,
        args: serde_json::Value,
        outcome: &str,
        code: Option<i32>,
        completed: bool,
    ) -> String {
        let turn = turn(state).await;
        state
            .record_tool_calls(
                &turn.turn_id,
                answer(),
                vec![ToolCall {
                    id: "result".into(),
                    name: name.into(),
                    arguments: args,
                }],
            )
            .await
            .unwrap();
        let message = state
            .record_tool_result_with_outcome(
                &turn.turn_id,
                "result",
                "saved real output".into(),
                Vec::new(),
                outcome,
                code,
            )
            .await
            .unwrap();
        let end = if completed {
            TurnOutcome::Completed(answer())
        } else {
            TurnOutcome::Interrupted(None)
        };
        state.finish_turn(&turn.turn_id, end).await.unwrap();
        format!("tool:{}", message.message_id)
    }
    async fn request(state: &State, goal: &Goal, id: &str) {
        let turn = turn(state).await;
        state
            .record_tool_calls(
                &turn.turn_id,
                answer(),
                vec![ToolCall {
                    id: "request".into(),
                    name: "goal".into(),
                    arguments: json!({"action":"request_verification"}),
                }],
            )
            .await
            .unwrap();
        state
            .request_goal_verification(
                "thread",
                &goal.revision,
                "request",
                vec![GoalEvidence {
                    criterion_id: "check".into(),
                    result_id: id.into(),
                }],
            )
            .await
            .unwrap();
        state
            .record_tool_result_with_outcome(
                &turn.turn_id,
                "request",
                "review requested".into(),
                Vec::new(),
                "succeeded",
                None,
            )
            .await
            .unwrap();
        state
            .finish_turn(&turn.turn_id, TurnOutcome::Completed(answer()))
            .await
            .unwrap();
    }
    fn command() -> GoalCheck {
        GoalCheck::Command {
            command: "cargo test --locked".into(),
        }
    }
    async fn proof(state: &State) -> String {
        result(
            state,
            "run_command",
            json!({"command":"cargo test --locked"}),
            "succeeded",
            Some(0),
            true,
        )
        .await
    }

    #[tokio::test]
    async fn only_runtime_can_complete_an_approved_all_pass_contract() {
        let (state, goal) = approved("goal-pass", command()).await;
        assert!(
            state
                .set_goal_status("thread", GoalStatus::Done, Some("model says done"))
                .await
                .is_err()
        );
        let id = proof(&state).await;
        request(&state, &goal, &id).await;
        assert_eq!(
            state.goal("thread").await.unwrap().unwrap().status,
            GoalStatus::Active
        );
        let reviewed = verify(&state, "thread").await.unwrap().unwrap();
        assert_eq!(reviewed.status, GoalStatus::Done);
        assert_eq!(reviewed.verification, "passed");
        assert!(
            verify(&state, "thread").await.unwrap().is_none(),
            "completion cannot reenter"
        );
    }

    #[tokio::test]
    async fn semantic_criterion_needs_confirmation_of_the_exact_current_result() {
        let (state, goal) = approved("goal-human", GoalCheck::UserConfirmation).await;
        let id = result(
            &state,
            "read_file",
            json!({"path":"artifact"}),
            "succeeded",
            None,
            true,
        )
        .await;
        request(&state, &goal, &id).await;
        let reviewed = verify(&state, "thread").await.unwrap().unwrap();
        assert_eq!(reviewed.status, GoalStatus::Active);
        assert_eq!(reviewed.verification, "needs_user");
        assert!(
            state
                .confirm_goal("thread", "stale", "check", &id)
                .await
                .is_err()
        );
        assert!(
            state
                .confirm_goal("thread", &goal.revision, "check", "tool:invented")
                .await
                .is_err()
        );
        state
            .confirm_goal("thread", &goal.revision, "check", &id)
            .await
            .unwrap();
        assert_eq!(
            verify(&state, "thread").await.unwrap().unwrap().status,
            GoalStatus::Done
        );
    }

    #[tokio::test]
    async fn failed_cancelled_unknown_nonzero_and_interrupted_results_never_pass() {
        for (index, outcome, code, completed) in [
            (0, "failed", Some(3), true),
            (1, "cancelled", None, true),
            (2, "unknown", Some(0), true),
            (3, "succeeded", Some(3), true),
            (4, "succeeded", Some(0), false),
            (5, "failed", None, true),
        ] {
            let (state, goal) = approved(&format!("goal-bad-{index}"), command()).await;
            let id = result(
                &state,
                "run_command",
                json!({"command":"cargo test --locked"}),
                outcome,
                code,
                completed,
            )
            .await;
            request(&state, &goal, &id).await;
            let reviewed = verify(&state, "thread").await.unwrap().unwrap();
            assert_eq!(reviewed.status, GoalStatus::Active, "{index}");
            assert_eq!(reviewed.verification, "gaps");
        }
    }

    #[tokio::test]
    async fn fabricated_foreign_wrong_command_and_old_revision_ids_do_not_pass() {
        let (state, goal) = approved("goal-identities", command()).await;
        let old = proof(&state).await;
        let draft = state.set_goal("thread", "new goal").await.unwrap();
        let draft = state
            .propose_goal("thread", &draft.revision, vec![criterion(command())])
            .await
            .unwrap();
        let current = state.approve_goal("thread", &draft.revision).await.unwrap();
        assert_ne!(current.revision, goal.revision);
        let wrong = result(
            &state,
            "run_command",
            json!({"command":"echo passed"}),
            "succeeded",
            Some(0),
            true,
        )
        .await;
        for id in ["tool:invented".to_owned(), old, wrong] {
            request(&state, &current, &id).await;
            let reviewed = verify(&state, "thread").await.unwrap().unwrap();
            assert_eq!(reviewed.status, GoalStatus::Active);
            assert_eq!(reviewed.verification, "gaps");
        }
        state.create_thread("foreign", "test").await.unwrap();
        // Even a valid execution cannot be reattributed to another owning thread.
        let actual_id = proof(&state).await;
        let epoch: i64 =
            sqlx::query_scalar("SELECT work_epoch FROM goals WHERE thread_id = 'thread'")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        let mut tx = state.pool.begin().await.unwrap();
        assert!(
            read_evidence(
                &mut tx,
                "thread",
                &current.revision,
                0,
                0,
                epoch,
                &actual_id
            )
            .await
            .unwrap()
            .is_some()
        );
        assert!(
            read_evidence(
                &mut tx,
                "foreign",
                &current.revision,
                0,
                0,
                epoch,
                &actual_id
            )
            .await
            .unwrap()
            .is_none()
        );
    }

    #[tokio::test]
    async fn later_changes_invalidate_proofs_within_the_same_revision() {
        let (state, goal) = approved("goal-stale-work", command()).await;
        let old = proof(&state).await;
        result(
            &state,
            "write_file",
            json!({"path":"src/lib.rs"}),
            "succeeded",
            None,
            true,
        )
        .await;
        request(&state, &goal, &old).await;
        assert_eq!(
            verify(&state, "thread")
                .await
                .unwrap()
                .unwrap()
                .verification,
            "gaps"
        );
        let fresh = proof(&state).await;
        request(&state, &goal, &fresh).await;
        assert_eq!(
            verify(&state, "thread").await.unwrap().unwrap().status,
            GoalStatus::Done
        );
    }

    async fn epoch(state: &State) -> i64 {
        sqlx::query_scalar("SELECT work_epoch FROM goals WHERE thread_id = 'thread'")
            .fetch_one(&state.pool)
            .await
            .unwrap()
    }

    async fn deliver_jobs(state: &State) {
        for pending in state.pending_messages("thread").await.unwrap() {
            state.admit_pending("thread", &pending).await.unwrap();
        }
        let delivery = state.start_turn("thread").await.unwrap().unwrap();
        state
            .finish_turn(&delivery.turn_id, TurnOutcome::Completed(answer()))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn paused_file_and_shell_mutations_invalidate_proofs_before_resume() {
        for (name, arguments) in [
            (
                "write_file",
                json!({"path":"src/lib.rs","content":"changed"}),
            ),
            (
                "run_command",
                json!({"command":"printf changed > src/lib.rs"}),
            ),
        ] {
            let (state, goal) = approved(&format!("goal-paused-{name}"), command()).await;
            let old = proof(&state).await;
            state
                .set_goal_status("thread", GoalStatus::Paused, None)
                .await
                .unwrap();
            let before = epoch(&state).await;
            result(&state, name, arguments, "succeeded", Some(0), true).await;
            assert!(
                epoch(&state).await > before,
                "paused mutations must invalidate immediately"
            );
            state
                .set_goal_status("thread", GoalStatus::Active, None)
                .await
                .unwrap();
            request(&state, &goal, &old).await;
            let reviewed = verify(&state, "thread").await.unwrap().unwrap();
            assert_eq!(reviewed.status, GoalStatus::Active);
            assert_eq!(reviewed.verification, "gaps");
            let fresh = proof(&state).await;
            request(&state, &goal, &fresh).await;
            assert_eq!(
                verify(&state, "thread").await.unwrap().unwrap().status,
                GoalStatus::Done
            );
        }
    }

    #[tokio::test]
    async fn resume_requires_fresh_proofs_even_without_a_recorded_mutation() {
        let (state, goal) = approved("goal-resume-fence", command()).await;
        let old = proof(&state).await;
        state
            .set_goal_status("thread", GoalStatus::Paused, None)
            .await
            .unwrap();
        let before = epoch(&state).await;
        state
            .set_goal_status("thread", GoalStatus::Active, None)
            .await
            .unwrap();
        assert!(epoch(&state).await > before);
        request(&state, &goal, &old).await;
        assert_eq!(
            verify(&state, "thread")
                .await
                .unwrap()
                .unwrap()
                .verification,
            "gaps"
        );
    }

    #[tokio::test]
    async fn a_check_job_started_before_resume_cannot_refresh_its_launch_epoch() {
        let (state, goal) = approved("goal-old-job-resume", command()).await;
        state
            .set_goal_status("thread", GoalStatus::Paused, None)
            .await
            .unwrap();
        state
            .create_job(
                "thread",
                "old-job",
                "cargo test --locked",
                "runtime",
                "/lease",
            )
            .await
            .unwrap();
        let before = epoch(&state).await;
        state
            .set_goal_status("thread", GoalStatus::Active, None)
            .await
            .unwrap();
        assert!(epoch(&state).await > before);
        state
            .finish_job("old-job", "runtime", "completed", "exit code: 0")
            .await
            .unwrap();
        deliver_jobs(&state).await;
        let launch: i64 =
            sqlx::query_scalar("SELECT goal_epoch FROM background_jobs WHERE job_id = 'old-job'")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert_eq!(
            launch, before,
            "completion must retain the original check epoch"
        );
        request(&state, &goal, "job:old-job").await;
        assert_eq!(
            verify(&state, "thread")
                .await
                .unwrap()
                .unwrap()
                .verification,
            "gaps"
        );
        state
            .create_job(
                "thread",
                "fresh-job",
                "cargo test --locked",
                "runtime",
                "/lease",
            )
            .await
            .unwrap();
        state
            .finish_job("fresh-job", "runtime", "completed", "exit code: 0")
            .await
            .unwrap();
        deliver_jobs(&state).await;
        request(&state, &goal, "job:fresh-job").await;
        assert_eq!(
            verify(&state, "thread").await.unwrap().unwrap().status,
            GoalStatus::Done
        );
    }

    #[tokio::test]
    async fn a_background_check_before_a_later_batch_mutation_keeps_its_own_epoch() {
        let (state, goal) = approved("goal-job-batch-order", command()).await;
        let turn = turn(&state).await;
        state
            .record_tool_calls(
                &turn.turn_id,
                answer(),
                vec![
                    ToolCall {
                        id: "check-job".into(),
                        name: "run_command".into(),
                        arguments: json!({"command":"cargo test --locked","background":true}),
                    },
                    ToolCall {
                        id: "edit-after".into(),
                        name: "write_file".into(),
                        arguments: json!({"path":"src/lib.rs"}),
                    },
                ],
            )
            .await
            .unwrap();
        assert!(
            state
                .create_job_for_call(
                    "thread",
                    "forged-job",
                    "cargo test --locked",
                    "runtime",
                    "/lease",
                    Some("invented")
                )
                .await
                .is_err()
        );
        state
            .create_job_for_call(
                "thread",
                "job",
                "cargo test --locked",
                "runtime",
                "/lease",
                Some("check-job"),
            )
            .await
            .unwrap();
        state
            .record_tool_result_with_outcome(
                &turn.turn_id,
                "check-job",
                "job started".into(),
                Vec::new(),
                "succeeded",
                None,
            )
            .await
            .unwrap();
        state
            .record_tool_result_with_outcome(
                &turn.turn_id,
                "edit-after",
                "changed".into(),
                Vec::new(),
                "succeeded",
                None,
            )
            .await
            .unwrap();
        state
            .finish_turn(&turn.turn_id, TurnOutcome::Completed(answer()))
            .await
            .unwrap();
        state
            .finish_job("job", "runtime", "completed", "exit code: 0")
            .await
            .unwrap();
        deliver_jobs(&state).await;
        let launch: i64 =
            sqlx::query_scalar("SELECT goal_epoch FROM background_jobs WHERE job_id = 'job'")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert!(launch < epoch(&state).await);
        request(&state, &goal, "job:job").await;
        assert_eq!(
            verify(&state, "thread")
                .await
                .unwrap()
                .unwrap()
                .verification,
            "gaps"
        );
    }

    #[tokio::test]
    async fn changing_jobs_invalidate_proofs_while_the_contract_is_paused() {
        let (state, goal) = approved("goal-paused-mutating-job", command()).await;
        let old = proof(&state).await;
        state
            .set_goal_status("thread", GoalStatus::Paused, None)
            .await
            .unwrap();
        let before = epoch(&state).await;
        state
            .create_job(
                "thread",
                "mutator",
                "printf changed > src/lib.rs",
                "runtime",
                "/lease",
            )
            .await
            .unwrap();
        state
            .finish_job("mutator", "runtime", "completed", "exit code: 0")
            .await
            .unwrap();
        assert!(epoch(&state).await > before);
        deliver_jobs(&state).await;
        state
            .set_goal_status("thread", GoalStatus::Active, None)
            .await
            .unwrap();
        request(&state, &goal, &old).await;
        assert_eq!(
            verify(&state, "thread")
                .await
                .unwrap()
                .unwrap()
                .verification,
            "gaps"
        );
    }

    #[tokio::test]
    async fn pending_input_running_dependency_and_unanswered_question_block_completion() {
        let (state, goal) = approved("goal-busy", command()).await;
        let id = proof(&state).await;
        request(&state, &goal, &id).await;
        let pending = state.add_pending("thread", "new user input").await.unwrap();
        assert!(verify(&state, "thread").await.unwrap().is_none());
        state.remove_pending(pending.pending_id).await.unwrap();
        state
            .create_job(
                "thread",
                "dependency",
                "cargo test --locked",
                "runtime",
                "/lease",
            )
            .await
            .unwrap();
        assert!(verify(&state, "thread").await.unwrap().is_none());
        state
            .remove_unstarted_job("dependency", "runtime")
            .await
            .unwrap();
        result(&state, "ask_user", json!({}), "succeeded", None, true).await;
        assert!(verify(&state, "thread").await.unwrap().is_none());
        state
            .set_goal_status("thread", GoalStatus::Paused, Some("cancelled"))
            .await
            .unwrap();
        assert!(verify(&state, "thread").await.unwrap().is_none());
        assert_eq!(
            state.goal("thread").await.unwrap().unwrap().status,
            GoalStatus::Paused
        );
    }

    #[tokio::test]
    async fn successful_jobs_use_the_approved_command_and_owner_revision() {
        let (state, goal) = approved("goal-job", command()).await;
        state
            .create_job("thread", "job", "cargo test --locked", "runtime", "/lease")
            .await
            .unwrap();
        state
            .finish_job("job", "runtime", "completed", "exit code: 0")
            .await
            .unwrap();
        let pending = state.pending_messages("thread").await.unwrap();
        state.admit_pending("thread", &pending[0]).await.unwrap();
        let delivery = state.start_turn("thread").await.unwrap().unwrap();
        state
            .finish_turn(&delivery.turn_id, TurnOutcome::Completed(answer()))
            .await
            .unwrap();
        request(&state, &goal, "job:job").await;
        assert_eq!(
            verify(&state, "thread").await.unwrap().unwrap().status,
            GoalStatus::Done
        );
    }

    #[tokio::test]
    async fn failed_timed_out_cancelled_and_interrupted_jobs_are_not_evidence() {
        for status in ["failed", "timed_out", "cancelled", "interrupted"] {
            let (state, goal) = approved(&format!("goal-job-{status}"), command()).await;
            state
                .create_job("thread", "job", "cargo test --locked", "runtime", "/lease")
                .await
                .unwrap();
            state
                .finish_job("job", "runtime", status, "no successful outcome")
                .await
                .unwrap();
            let pending = state.pending_messages("thread").await.unwrap();
            state.admit_pending("thread", &pending[0]).await.unwrap();
            let delivery = state.start_turn("thread").await.unwrap().unwrap();
            state
                .finish_turn(&delivery.turn_id, TurnOutcome::Completed(answer()))
                .await
                .unwrap();
            request(&state, &goal, "job:job").await;
            assert_eq!(
                verify(&state, "thread")
                    .await
                    .unwrap()
                    .unwrap()
                    .verification,
                "gaps"
            );
        }
    }

    #[tokio::test]
    async fn review_timeout_persists_a_gap_and_never_a_pass() {
        let (state, goal) = approved("goal-review-timeout", command()).await;
        let id = proof(&state).await;
        request(&state, &goal, &id).await;
        let (revision, attempt) = begin(&state, "thread").await.unwrap().unwrap();
        // Hold the only connection so evidence review cannot finish before its bound.
        let held = state.pool.acquire().await.unwrap();
        let release = async {
            tokio::time::sleep(Duration::from_millis(40)).await;
            drop(held);
        };
        let review = finish_review(
            &state,
            "thread",
            revision,
            attempt,
            Duration::from_millis(5),
        );
        let (_, reviewed) = tokio::join!(release, review);
        let reviewed = reviewed.unwrap().unwrap();
        assert_eq!(reviewed.status, GoalStatus::Active);
        assert_eq!(reviewed.verification, "gaps");
        assert!(reviewed.verification_note.unwrap().contains("timed out"));
    }

    #[tokio::test]
    async fn interrupted_verification_recovery_and_attempt_limit_never_yield_pass() {
        let (state, goal) = approved("goal-recovery", command()).await;
        let id = proof(&state).await;
        request(&state, &goal, &id).await;
        sqlx::query("UPDATE goals SET verification = 'checking' WHERE thread_id = 'thread'")
            .execute(&state.pool)
            .await
            .unwrap();
        state.recover_goal_verification("thread").await.unwrap();
        let recovered = state.goal("thread").await.unwrap().unwrap();
        assert_eq!(recovered.verification, "gaps");
        assert_eq!(recovered.status, GoalStatus::Active);
        assert!(verify(&state, "thread").await.unwrap().is_none());
        for _ in 0..3 {
            request(&state, &goal, "tool:missing").await;
            assert_eq!(
                verify(&state, "thread")
                    .await
                    .unwrap()
                    .unwrap()
                    .verification,
                "gaps"
            );
        }
        let turn = turn(&state).await;
        state
            .record_tool_calls(
                &turn.turn_id,
                answer(),
                vec![ToolCall {
                    id: "last".into(),
                    name: "goal".into(),
                    arguments: json!({}),
                }],
            )
            .await
            .unwrap();
        assert!(
            state
                .request_goal_verification(
                    "thread",
                    &goal.revision,
                    "last",
                    vec![GoalEvidence {
                        criterion_id: "check".into(),
                        result_id: id
                    }]
                )
                .await
                .is_err()
        );
    }
}
