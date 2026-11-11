//! What blocks completion: pending input, jobs that fail, a review that times out.

use super::*;

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
