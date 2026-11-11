use super::evidence::read_evidence;
use super::*;
use crate::state::{Goal, GoalCheck, GoalCriterion, GoalEvidence};
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
    let epoch: i64 = sqlx::query_scalar("SELECT work_epoch FROM goals WHERE thread_id = 'thread'")
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
        "files__write_file",
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
            "files__write_file",
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
                    name: "files__write_file".into(),
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
